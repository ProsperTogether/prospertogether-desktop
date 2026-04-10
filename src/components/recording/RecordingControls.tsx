import { useState, useEffect, useCallback, useRef } from 'react';
import { useNavigate } from 'react-router-dom';
import { listen, emit } from '@tauri-apps/api/event';
import {
  getCurrentWindow,
  currentMonitor,
  LogicalSize,
  LogicalPosition,
  type PhysicalPosition,
  type PhysicalSize,
} from '@tauri-apps/api/window';

import { AudioLevelMeter } from '../audio/AudioLevelMeter';
import { FloatingToolbar } from '../overlay/FloatingToolbar';
import { ScreenPicker } from './ScreenPicker';
import { WindowPicker } from './WindowPicker';
import { RegionPicker } from './RegionPicker';
import { useRecordingStore } from '../../store/recordingStore';
import type { DrawTool, DrawColor } from '../overlay/types';
import type { CaptureMode, CaptureTarget } from '../../types/capture';

interface AudioDevice {
  id: string;
  name: string;
}

const DOCK_WIDTH = 1280;
const DOCK_HEIGHT = 84;

interface BackendRecordingState {
  active: boolean;
  file_path: string | null;
  started_at_ms: number | null;
  duration_seconds: number;
}

export const RecordingControls = () => {
  const navigate = useNavigate();
  const { status, duration, setStatus, setDuration, setStartedAtMs, reset, selectedAudio, setSelectedAudio, captureTarget } = useRecordingStore();
  const [countdown, setCountdown] = useState(3);
  const [error, setError] = useState<string | null>(null);
  const [audioDevices, setAudioDevices] = useState<AudioDevice[]>([]);

  // Drawing-tool state — owned by the dock, pushed to the overlay window via events
  const [drawingActive, setDrawingActive] = useState(false);
  const [tool, setTool] = useState<DrawTool>('pen');
  const [color, setColor] = useState<DrawColor>('#ef4444');
  const [strokeWidth, setStrokeWidth] = useState(4);

  const [activeTab, setActiveTab] = useState<CaptureMode>('screen');

  // Saved pre-recording window bounds for restoration on stop / unmount
  const prevWindowBoundsRef = useRef<{
    size: PhysicalSize;
    position: PhysicalPosition;
  } | null>(null);

  // Guard for the on-mount recovery check (defined further down, after the
  // collapseToDock useCallback it depends on). Declared here so it survives
  // re-renders without being re-initialized.
  const recoveryAttemptedRef = useRef(false);

  useEffect(() => {
    import('@tauri-apps/api/core')
      .then(core => core.invoke<AudioDevice[]>('list_audio_devices'))
      .then(devices => {
        setAudioDevices(devices);
        import('@tauri-apps/plugin-store')
          .then(({ load }) => load('settings.json'))
          .then(store => store.get<string>('preferred_audio_device'))
          .then(saved => {
            if (saved && devices.some(d => d.id === saved)) {
              setSelectedAudio(saved);
            } else if (devices.length > 0) {
              setSelectedAudio(devices[0].id);
            }
          })
          .catch(() => {
            if (devices.length > 0) setSelectedAudio(devices[0].id);
          });
      })
      .catch(() => {});
  }, [setSelectedAudio]);

  // Collapse the main window into a top-center horizontal dock when recording starts.
  // Restore the original size/position when recording ends or the component unmounts.
  // Each step is wrapped in its own try/catch so a single failing call (e.g. if a
  // capability isn't granted) doesn't prevent the rest of the resize from happening.
  const collapseToDock = useCallback(async () => {
    const win = getCurrentWindow();

    // Capture pre-recording bounds for restoration
    try {
      if (!prevWindowBoundsRef.current) {
        prevWindowBoundsRef.current = {
          size: await win.outerSize(),
          position: await win.outerPosition(),
        };
      }
    } catch (err) {
      console.warn('[RecordingControls] outerSize/outerPosition failed:', err);
    }

    // Compute target position centered on the active monitor
    let x = 0;
    let y = 0;
    try {
      const monitor = await currentMonitor();
      if (monitor) {
        const sf = monitor.scaleFactor || 1;
        const monitorWidthLogical = monitor.size.width / sf;
        const monitorXLogical = monitor.position.x / sf;
        const monitorYLogical = monitor.position.y / sf;
        x = monitorXLogical + (monitorWidthLogical - DOCK_WIDTH) / 2;
        y = monitorYLogical + 8;
      }
    } catch (err) {
      console.warn('[RecordingControls] currentMonitor failed:', err);
    }

    // Each window mutation in its own try block — a failure on one shouldn't
    // abort the rest. Order: decorations off → size → position → always-on-top.
    try { await win.setDecorations(false); } catch (err) {
      console.warn('[RecordingControls] setDecorations(false) failed:', err);
    }
    try { await win.setResizable(false); } catch (err) {
      console.warn('[RecordingControls] setResizable(false) failed:', err);
    }
    try { await win.setSize(new LogicalSize(DOCK_WIDTH, DOCK_HEIGHT)); } catch (err) {
      console.warn('[RecordingControls] setSize failed:', err);
    }
    try { await win.setPosition(new LogicalPosition(x, y)); } catch (err) {
      console.warn('[RecordingControls] setPosition failed:', err);
    }
    try { await win.setAlwaysOnTop(true); } catch (err) {
      console.warn('[RecordingControls] setAlwaysOnTop(true) failed:', err);
    }
  }, []);

  const restoreWindow = useCallback(async () => {
    const win = getCurrentWindow();
    try { await win.setAlwaysOnTop(false); } catch (err) {
      console.warn('[RecordingControls] setAlwaysOnTop(false) failed:', err);
    }
    try { await win.setDecorations(true); } catch (err) {
      console.warn('[RecordingControls] setDecorations(true) failed:', err);
    }
    try { await win.setResizable(true); } catch (err) {
      console.warn('[RecordingControls] setResizable(true) failed:', err);
    }
    const prev = prevWindowBoundsRef.current;
    if (prev) {
      try { await win.setSize(prev.size); } catch (err) {
        console.warn('[RecordingControls] restore setSize failed:', err);
      }
      try { await win.setPosition(prev.position); } catch (err) {
        console.warn('[RecordingControls] restore setPosition failed:', err);
      }
    }
    prevWindowBoundsRef.current = null;
  }, []);

  // On mount: ask the backend whether a recording is currently in progress
  // and resume into the dock UI if so. This handles three classes of "ghost
  // recording" — an ffmpeg.exe writing to disk while the UI thinks it's idle:
  //   1. A previous Tauri process was killed mid-recording. Its ffmpeg was
  //      orphaned (Windows doesn't auto-kill grandchildren) and the new
  //      Rust process recovered the PID via the disk state file in setup().
  //   2. The frontend was hot-reloaded or remounted while a recording was
  //      in progress (Rust state survives, frontend Zustand state is lost).
  //   3. A start_recording race left the Rust state ahead of the UI.
  // If no recording is active, we do the normal idle reset instead.
  // NOTE: this effect must be placed AFTER `collapseToDock` is declared
  // (above) because it references it in its dependency array, and `const`
  // declarations are in the temporal dead zone before initialization.
  useEffect(() => {
    if (recoveryAttemptedRef.current) return;
    recoveryAttemptedRef.current = true;

    // Synchronously clear "finalizing" stale state (processing/uploading)
    // that should never survive a fresh mount. Don't touch in-progress
    // states ('recording'/'countdown'): if a hot reload happened while a
    // recording was active, the store correctly says 'recording' and the
    // async recovery below will reconcile with the Rust state. Blasting
    // 'recording' to 'idle' here would cause a UI flash.
    const stale = useRecordingStore.getState().status;
    if (stale === 'processing' || stale === 'uploading') {
      reset();
    }

    let cancelled = false;
    (async () => {
      try {
        const core = await import('@tauri-apps/api/core');
        const rec = await core.invoke<BackendRecordingState>('get_recording_state');
        if (cancelled) return;

        if (rec.active) {
          console.log('[RecordingControls] resuming active recording', rec);
          setStartedAtMs(rec.started_at_ms);
          setDuration(rec.duration_seconds);
          // Only collapse if we're not already showing the dock UI. After
          // hot reload mid-recording the window may already be at dock
          // size; collapsing again would save the dock bounds as the
          // "previous" bounds and break restore on stop.
          if (useRecordingStore.getState().status !== 'recording') {
            await collapseToDock();
            if (cancelled) return;
            setStatus('recording');
          }
        } else {
          // Rust says no recording in progress. Make sure the frontend
          // store agrees — clears any leftover non-finalizing stale state
          // (e.g. zombie 'recording' from a process that crashed).
          if (useRecordingStore.getState().status !== 'idle') {
            reset();
          }
        }
      } catch (err) {
        console.warn('[RecordingControls] recovery check failed:', err);
        // On IPC failure assume idle to avoid leaving the user stuck in
        // a stale finalizing state.
        if (useRecordingStore.getState().status !== 'idle') {
          reset();
        }
      }
    })();

    return () => { cancelled = true; };
  }, [reset, setStatus, setDuration, setStartedAtMs, collapseToDock]);

  // Reset the countdown to 3 when ENTERING the countdown state. This used to
  // be done inside the interval (`return 3` after firing start_recording), but
  // that caused a visible flash back to "3" while waiting for the async IPC +
  // window resize to complete, before the dock UI took over.
  useEffect(() => {
    if (status === 'countdown') setCountdown(3);
  }, [status]);

  // Guard against React 18 StrictMode double-invocation of state updaters,
  // which would otherwise fire start_recording twice — the first call wins
  // (ffmpeg starts, recording_active=true) and the second crashes with
  // "Recording already in progress", leaving a ghost recording running.
  const startInFlightRef = useRef(false);

  useEffect(() => {
    if (status === 'countdown') {
      // Pure updater: only computes the next countdown value. Side effects
      // (start_recording IPC) are scheduled OUTSIDE the updater, after the
      // setState call returns, so StrictMode's double-invoke does not
      // double-fire the IPC.
      const timer = setInterval(() => {
        setCountdown(prev => (prev <= 1 ? 1 : prev - 1));
      }, 1000);
      return () => clearInterval(timer);
    }
  }, [status]);

  // Trigger start_recording exactly once when the countdown reaches 1 while
  // in countdown state. Dedupe via ref so StrictMode's double-effect-run on
  // mount cannot fire it twice either.
  useEffect(() => {
    if (status !== 'countdown' || countdown !== 1) return;
    if (startInFlightRef.current) return;
    startInFlightRef.current = true;

    let cancelled = false;
    (async () => {
      try {
        const core = await import('@tauri-apps/api/core');
        await core.invoke('start_recording', {
          target: captureTarget,
          audio: selectedAudio ?? '',
        });
        if (cancelled) return;
        // Mark the canonical wall-clock start. Used by the duration display
        // and by the recovery flow if the frontend later loses state.
        setStartedAtMs(Date.now());
        setDuration(0);
        // Collapse the main window to the top dock BEFORE flipping to
        // 'recording' state so the user sees the dock UI immediately,
        // not a flash of fullscreen-dark. NO overlay window is created
        // here — drawing is opt-in via the dock toggle.
        await collapseToDock();

        // Border overlay for ALL visual capture modes. The Rust WGC path
        // used to draw its own DWM border for window mode, but we switched
        // to monitor-capture-plus-crop to fix drawing-overlay capture and
        // frame rate issues (see commands/window_capture.rs), and monitor
        // capture has no per-window border. So for window mode we render
        // our own Tauri border overlay, which `watch_window_rect` keeps
        // repositioned via `border:reposition` events. The overlay is
        // drawn OUTSIDE the window rect (BORDER_WIDTH pixels larger on
        // every side) so it stays clear of the crop rectangle and is NOT
        // itself captured into the recording.
        // Border overlay only for region/monitor modes. Window mode gets
        // the DWM-drawn yellow border from the second WGC session in
        // window_capture.rs — drawing our own on top of that looks
        // janky and doesn't add anything.
        if (captureTarget.mode === 'region' || captureTarget.mode === 'monitor') {
          try {
            const { createBorderOverlay } = await import('../../overlay/useBorderOverlay');
            await createBorderOverlay(captureTarget);
          } catch (err) {
            console.warn('[RecordingControls] border overlay failed:', err);
          }
        }

        // For window mode: start polling the target window's rect and
        // emitting `border:reposition` events. The drawing overlay
        // (if the user enables drawing) listens to these events and
        // repositions itself so annotations follow the moving window.
        // WGC captures the window content directly, so the recording
        // itself already follows — this is just for the drawing overlay.
        if (captureTarget.mode === 'window') {
          try {
            const core2 = await import('@tauri-apps/api/core');
            await core2.invoke('watch_window_rect', { hwnd: captureTarget.hwnd });
          } catch (err) {
            console.warn('[RecordingControls] watch_window_rect failed:', err);
          }
        }

        setStatus('recording');
      } catch (err) {
        if (cancelled) return;
        // Defense in depth: if start_recording fails because Rust already
        // has a recording in progress (a race we shouldn't be hitting now
        // that the StrictMode-double-fire bug is fixed, but be paranoid),
        // recover into that recording instead of showing an error.
        const msg = String(err);
        if (msg.includes('Recording already in progress')) {
          try {
            const core = await import('@tauri-apps/api/core');
            const rec = await core.invoke<BackendRecordingState>('get_recording_state');
            if (!cancelled && rec.active) {
              console.warn('[RecordingControls] start race — resuming existing recording', rec);
              setStartedAtMs(rec.started_at_ms);
              setDuration(rec.duration_seconds);
              await collapseToDock();
              if (!cancelled) setStatus('recording');
              return;
            }
          } catch (recoveryErr) {
            console.warn('[RecordingControls] race-recovery failed:', recoveryErr);
          }
        }
        setError(msg);
        setStatus('idle');
      } finally {
        startInFlightRef.current = false;
      }
    })();

    return () => { cancelled = true; };
  }, [status, countdown, selectedAudio, captureTarget, collapseToDock, setStatus, setStartedAtMs, setDuration]);

  useEffect(() => {
    if (status !== 'recording') return;
    // Read the latest duration from the store inside the callback so this effect
    // does not depend on `duration` — otherwise the interval is torn down and
    // recreated every second, causing extra renders during the recording hot path.
    const timer = setInterval(() => {
      setDuration(useRecordingStore.getState().duration + 1);
    }, 1000);
    return () => clearInterval(timer);
  }, [status, setDuration]);

  const formatTime = (seconds: number) => {
    const m = Math.floor(seconds / 60).toString().padStart(2, '0');
    const s = (seconds % 60).toString().padStart(2, '0');
    return `${m}:${s}`;
  };

  const handleStart = useCallback(() => {
    setError(null);
    setStatus('countdown');
  }, [setStatus]);

  // Toggle drawing on/off — creates or destroys the annotation overlay window.
  // The overlay only exists while drawing is active, so the lockout cannot
  // happen in the default recording flow.
  const handleToggleDrawing = useCallback(async () => {
    if (drawingActive) {
      try {
        const { destroyOverlayWindow } = await import('../../overlay/useOverlayWindow');
        await destroyOverlayWindow();
      } catch (err) {
        console.warn('[RecordingControls] destroyOverlayWindow failed:', err);
      }
      setDrawingActive(false);
    } else {
      try {
        const { createOverlayWindow } = await import('../../overlay/useOverlayWindow');
        // For window/region mode, constrain drawing to capture area
        const captureRect = (captureTarget.mode === 'window') ? captureTarget.rect
          : (captureTarget.mode === 'region' || captureTarget.mode === 'monitor') ? captureTarget
          : undefined;
        await createOverlayWindow(captureRect);
        // Push current tool/color/width state to the freshly-created overlay
        await emit('overlay:set-tool', tool);
        await emit('overlay:set-color', color);
        await emit('overlay:set-stroke-width', strokeWidth);
        setDrawingActive(true);
      } catch (err) {
        console.error('[RecordingControls] createOverlayWindow failed:', err);
        setError(`Failed to start drawing: ${err}`);
      }
    }
  }, [drawingActive, captureTarget, tool, color, strokeWidth]);

  // When dock controls change, push the new value to the overlay window
  const handleToolChange = useCallback((next: DrawTool) => {
    setTool(next);
    if (drawingActive) void emit('overlay:set-tool', next);
  }, [drawingActive]);

  const handleColorChange = useCallback((next: DrawColor) => {
    setColor(next);
    if (drawingActive) void emit('overlay:set-color', next);
  }, [drawingActive]);

  const handleStrokeWidthChange = useCallback((next: number) => {
    setStrokeWidth(next);
    if (drawingActive) void emit('overlay:set-stroke-width', next);
  }, [drawingActive]);

  const handleUndo = useCallback(() => {
    if (drawingActive) void emit('overlay:undo');
  }, [drawingActive]);

  const handleRedo = useCallback(() => {
    if (drawingActive) void emit('overlay:redo');
  }, [drawingActive]);

  const handleClear = useCallback(() => {
    if (drawingActive) void emit('overlay:clear');
  }, [drawingActive]);

  const handleStop = useCallback(async () => {
    // Show "Finalizing" UI immediately so the user sees something happening
    // while stop_recording (which can take several seconds) runs in the
    // background. This flips the render branch from the dock to the full
    // finalizing screen.
    setStatus('processing');
    setError(null);
    setStartedAtMs(null);

    // Destroy border overlay if any
    try {
      const { destroyBorderOverlay } = await import('../../overlay/useBorderOverlay');
      await destroyBorderOverlay();
    } catch { /* may not exist */ }

    // Stop watching window rect if any
    try {
      const core = await import('@tauri-apps/api/core');
      await core.invoke('stop_watching_window');
    } catch { /* may not be watching */ }

    // Destroy drawing overlay while still recording so final strokes capture
    try {
      const { destroyOverlayWindow } = await import('../../overlay/useOverlayWindow');
      await destroyOverlayWindow();
    } catch { /* overlay may not exist */ }
    setDrawingActive(false);

    // Restore the main window from dock to its normal size/position BEFORE
    // we navigate. Otherwise the ReviewPage renders inside the 84px dock.
    await restoreWindow();

    try {
      const core = await import('@tauri-apps/api/core');
      const meta = await core.invoke<{ id: string }>('stop_recording');
      // Reset local store state before navigating.
      useRecordingStore.getState().reset();
      navigate(`/review/${meta.id}`);
    } catch (err) {
      setError(`Stop failed: ${err}`);
      // Reset so the user can try again from the idle screen.
      useRecordingStore.getState().reset();
    }
  }, [navigate, restoreWindow, setStartedAtMs, setStatus]);

  const handleSwitchTarget = useCallback(async (newTarget: CaptureTarget) => {
    try {
      const core = await import('@tauri-apps/api/core');

      // Stop old window watcher if was tracking a window
      if (captureTarget.mode === 'window') {
        try { await core.invoke('stop_watching_window'); } catch {}
      }

      // Destroy old border overlay
      try {
        const { destroyBorderOverlay } = await import('../../overlay/useBorderOverlay');
        await destroyBorderOverlay();
      } catch {}

      // If drawing is active, destroy and recreate overlay for new target
      if (drawingActive) {
        try {
          const { destroyOverlayWindow } = await import('../../overlay/useOverlayWindow');
          await destroyOverlayWindow();
        } catch {}
        setDrawingActive(false);
      }

      // Switch capture target (stops old ffmpeg, starts new)
      await core.invoke('switch_capture_target', {
        target: newTarget,
        audio: selectedAudio ?? '',
      });

      // Update store
      useRecordingStore.getState().setCaptureTarget(newTarget);

      // Border overlay only for region/monitor modes. Window mode gets
      // the DWM-drawn yellow border from the dual WGC session.
      if (newTarget.mode === 'region' || newTarget.mode === 'monitor') {
        try {
          const { createBorderOverlay } = await import('../../overlay/useBorderOverlay');
          await createBorderOverlay(newTarget);
        } catch (err) {
          console.warn('[RecordingControls] border overlay failed:', err);
        }
      }

      // For window mode: start polling the new target window's rect so
      // the drawing overlay can follow it (if drawing is later enabled).
      if (newTarget.mode === 'window') {
        try {
          await core.invoke('watch_window_rect', { hwnd: newTarget.hwnd });
        } catch (err) {
          console.warn('[RecordingControls] watch_window_rect failed:', err);
        }
      }
    } catch (err) {
      console.error('[RecordingControls] switch target failed:', err);
      setError(`Failed to switch target: ${err}`);
    }
  }, [captureTarget, drawingActive, selectedAudio]);

  const handleOpenDockPicker = useCallback(async () => {
    try {
      const { openDockPicker } = await import('../../overlay/useDockPicker');
      const newTarget = await openDockPicker();
      if (newTarget) {
        await handleSwitchTarget(newTarget);
      }
    } catch (err) {
      console.error('[RecordingControls] openDockPicker failed:', err);
    }
  }, [handleSwitchTarget]);

  // Listen for global Ctrl+Shift+S stop shortcut + tray menu Stop Recording
  const handleStopRef = useRef(handleStop);
  handleStopRef.current = handleStop;
  useEffect(() => {
    if (status !== 'recording') return;
    let unlisten: (() => void) | undefined;
    listen('recording:stop', () => {
      handleStopRef.current();
    }).then(u => { unlisten = u; });
    return () => { unlisten?.(); };
  }, [status]);

  // Cleanup: if the component unmounts mid-recording (navigation, hot reload),
  // make sure the dock + overlay don't leak across to the next mount.
  useEffect(() => {
    return () => {
      void (async () => {
        try {
          const { destroyOverlayWindow } = await import('../../overlay/useOverlayWindow');
          await destroyOverlayWindow();
        } catch { /* may not exist */ }
        try {
          const { destroyBorderOverlay } = await import('../../overlay/useBorderOverlay');
          await destroyBorderOverlay();
        } catch { /* may not exist */ }
        try {
          const core = await import('@tauri-apps/api/core');
          await core.invoke('stop_watching_window');
        } catch { /* may not be watching */ }
        await restoreWindow();
      })();
    };
  }, [restoreWindow]);

  // ─── DOCK UI (during recording) ────────────────────────────────────────────
  if (status === 'recording') {
    return (
      <div
        data-tauri-drag-region
        className="fixed top-0 left-0 right-0 flex items-center gap-3 px-4 bg-slate-900/95 backdrop-blur border-b border-white/10 select-none"
        style={{ height: DOCK_HEIGHT, zIndex: 9999 }}
      >
        {/* Recording indicator + timer */}
        <div className="flex items-center gap-2 pr-3 border-r border-white/10" data-tauri-drag-region>
          <div className="w-2.5 h-2.5 rounded-full bg-red-500 animate-pulse shadow-lg shadow-red-500/60" />
          <span className="text-[10px] uppercase font-bold tracking-widest text-red-400">REC</span>
          <span className="text-xl font-mono font-light text-white tabular-nums tracking-wider ml-1">
            {formatTime(duration)}
          </span>
        </div>

        {/* Capture target indicator */}
        <div className="relative pr-3 border-r border-white/10">
          <button
            onClick={handleOpenDockPicker}
            className="flex items-center gap-1.5 px-2 py-1 rounded-md hover:bg-white/10 transition text-[12px]"
          >
            <span className="text-slate-400">
              {captureTarget.mode === 'screen' ? '🖥' : captureTarget.mode === 'window' ? '📺' : captureTarget.mode === 'region' ? '📐' : '🖥'}
            </span>
            <span className="text-white/80 max-w-[240px] truncate">
              {captureTarget.mode === 'window' ? captureTarget.title
                : captureTarget.mode === 'region' ? `${captureTarget.width}×${captureTarget.height} region`
                : captureTarget.mode === 'monitor' ? `Monitor ${captureTarget.width}×${captureTarget.height}`
                : 'Entire Screen'}
            </span>
            <svg className="w-3 h-3 text-slate-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M19 9l-7 7-7-7" />
            </svg>
          </button>
        </div>

        {/*
          Note: AudioLevelMeter intentionally NOT rendered during recording.
          ffmpeg already holds the dshow audio device for capture; opening the
          same mic via getUserMedia from the webview causes Windows to renegotiate
          exclusive device access on every frame, which manifests as PC lag and
          a flashing mouse cursor. The meter only belongs on the idle setup screen.
        */}

        {/* Drawing toolbar — pen/highlighter/arrow/rect, colors, stroke width,
            undo/redo/clear, and the Start/Stop Drawing toggle. The toggle is
            the gating control: until pressed, no overlay window exists. */}
        <FloatingToolbar
          tool={tool}
          color={color}
          strokeWidth={strokeWidth}
          drawingActive={drawingActive}
          onToolChange={handleToolChange}
          onColorChange={handleColorChange}
          onStrokeWidthChange={handleStrokeWidthChange}
          onUndo={handleUndo}
          onRedo={handleRedo}
          onClear={handleClear}
          onToggleDrawing={handleToggleDrawing}
        />

        {/* Stop Recording — pushed to the right */}
        <div className="flex-1" data-tauri-drag-region />
        <button
          onClick={handleStop}
          className="px-4 py-1.5 bg-red-500 hover:bg-red-600 text-white text-[12px] font-semibold rounded-md shadow-lg shadow-red-500/30 transition"
        >
          Stop Recording
        </button>
      </div>
    );
  }

  // ─── COUNTDOWN UI (full screen flash before recording starts) ──────────────
  if (status === 'countdown') {
    return (
      <div className="fixed inset-0 z-50 bg-slate-950 flex flex-col items-center justify-center">
        <p className="text-8xl font-bold text-white animate-pulse tabular-nums">{countdown}</p>
        <p className="text-slate-500 mt-3 text-sm">Get ready...</p>
      </div>
    );
  }

  // ─── FINALIZING UI (shown while stop_recording IPC runs) ──────────────────
  // The IPC waits for ffmpeg to finish writing the file, then runs another
  // ffmpeg pass for the thumbnail — can take 3-5 seconds total. Without a
  // visible indicator the user is left staring at a stale idle screen.
  if (status === 'processing' || status === 'uploading') {
    return (
      <div className="flex flex-col items-center justify-center py-16 px-6">
        <div className="w-12 h-12 border-4 border-slate-200 border-t-red-500 rounded-full animate-spin mb-5" />
        <p className="text-lg font-medium text-slate-900 mb-1">Finalizing recording</p>
        <p className="text-[13px] text-slate-500 text-center max-w-sm">
          Saving the video and generating a preview. This usually takes a few seconds.
        </p>
        {error && (
          <div className="mt-6 rounded-lg bg-red-50 border border-red-100 px-3.5 py-2.5 max-w-sm">
            <p className="text-[13px] text-red-600">{error}</p>
          </div>
        )}
      </div>
    );
  }

  // ─── IDLE UI (Ready to Record) ─────────────────────────────────────────────
  return (
    <div className="flex flex-col items-center justify-center py-10">
      <div className="px-6 w-full max-w-lg mx-auto">
        <div className="text-center mb-5">
          <div className="w-14 h-14 rounded-2xl bg-red-500/10 flex items-center justify-center mx-auto mb-4">
            <svg className="w-7 h-7 text-red-500" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
              <circle cx="12" cy="12" r="10" />
              <circle cx="12" cy="12" r="4" fill="currentColor" />
            </svg>
          </div>
          <h2 className="text-xl font-semibold text-slate-900 mb-1">Ready to Record</h2>
        </div>

        {error && (
          <div className="rounded-lg bg-red-50 border border-red-100 px-3.5 py-2.5 mb-4">
            <p className="text-[13px] text-red-600">{error}</p>
          </div>
        )}

        {/* Mode tabs */}
        <div className="flex rounded-lg bg-slate-100 p-1 mb-4">
          {(['screen', 'window', 'region'] as CaptureMode[]).map((mode) => (
            <button
              key={mode}
              onClick={() => setActiveTab(mode)}
              className={`flex-1 px-3 py-1.5 text-[13px] font-medium rounded-md transition ${
                activeTab === mode
                  ? 'bg-white text-slate-900 shadow-sm'
                  : 'text-slate-500 hover:text-slate-700'
              }`}
            >
              {mode === 'screen' ? 'Screen' : mode === 'window' ? 'Window' : 'Region'}
            </button>
          ))}
        </div>

        {/* Picker content */}
        <div className="mb-4">
          {activeTab === 'screen' && <ScreenPicker />}
          {activeTab === 'window' && <WindowPicker />}
          {activeTab === 'region' && <RegionPicker />}
        </div>

        {/* Audio selector */}
        {audioDevices.length > 0 && (
          <div className="mb-5 text-left">
            <label className="block text-[12px] font-medium text-slate-500 mb-1.5 uppercase tracking-wide">Audio Source</label>
            <select
              value={selectedAudio ?? ''}
              onChange={(e) => {
                setSelectedAudio(e.target.value || null);
                import('@tauri-apps/plugin-store')
                  .then(({ load }) => load('settings.json'))
                  .then(store => {
                    store.set('preferred_audio_device', e.target.value);
                    store.save();
                  })
                  .catch(() => {});
              }}
              className="w-full bg-white text-sm text-slate-900 border border-slate-200 rounded-lg px-3.5 py-2.5 focus:border-brand-500 focus:ring-2 focus:ring-brand-500/20 outline-none transition"
            >
              {audioDevices.map(d => (
                <option key={d.id} value={d.id}>{d.name}</option>
              ))}
              <option value="none">No Audio (screen only)</option>
            </select>
            {selectedAudio && selectedAudio !== 'none' && (
              <div className="mt-3">
                <AudioLevelMeter active />
              </div>
            )}
          </div>
        )}

        {/* Start button - disabled when no valid target for window/region */}
        <button
          onClick={handleStart}
          disabled={
            (activeTab === 'window' && captureTarget.mode !== 'window') ||
            (activeTab === 'region' && captureTarget.mode !== 'region')
          }
          className="w-full px-6 py-3 bg-gradient-to-b from-red-500 to-red-600 text-white text-[15px] font-semibold rounded-xl hover:from-red-600 hover:to-red-700 shadow-lg shadow-red-500/25 transition-all duration-200 hover:scale-[1.01] disabled:opacity-50 disabled:cursor-not-allowed disabled:hover:scale-100"
        >
          Start Recording
        </button>
        <button
          onClick={() => navigate('/')}
          className="mt-3 w-full text-[13px] text-slate-400 hover:text-slate-600 transition"
        >
          Cancel
        </button>
      </div>
    </div>
  );
};

