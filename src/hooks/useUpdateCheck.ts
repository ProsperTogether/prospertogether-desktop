import { useEffect, useState, useCallback, useRef } from 'react';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { invoke } from '@tauri-apps/api/core';

/**
 * Update state machine. Mirrors DevManager's updater stages adapted for the
 * Tauri 2 plugin-updater JS API, which has a simpler surface (check + download
 * + install are explicit calls rather than a callback-based session).
 */
export type UpdateStage =
  | 'idle'           // no check run yet or last check said up-to-date
  | 'checking'       // fetching manifest
  | 'available'      // manifest says a newer version is out; auto-download next
  | 'downloading'    // downloading .nsis.zip or .app.tar.gz
  | 'ready'          // download finished + signature verified; restart to install
  | 'installing'     // install triggered; app will relaunch
  | 'error';         // network / signature / install failure

/** Background check interval when no update is pending. */
const BACKGROUND_INTERVAL_MS = 30 * 60 * 1000; // 30 minutes
/** Shorter re-check interval when a download is ready to install. */
const READY_INTERVAL_MS = 5 * 60 * 1000; // 5 minutes

interface BackendRecordingGuard {
  canUpdate: boolean;
  recording: boolean;
  uploading: boolean;
  transcribing: boolean;
}

/**
 * Manages the auto-update lifecycle: startup check + periodic background
 * checks + auto-download on newer-version-found. Respects the existing
 * `check_for_update` Rust guard so we never install while a recording /
 * upload / transcription is in progress.
 *
 * Exposes `stage`, `availableVersion`, `error`, and an `installNow()`
 * trigger for a restart-and-install button.
 */
export function useUpdateCheck() {
  const [stage, setStage] = useState<UpdateStage>('idle');
  const [availableVersion, setAvailableVersion] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  // The Update object returned by check() carries the downloadAndInstall
  // method on it. We stash it between the download and install phases so
  // the user's "Restart to install" click can trigger the final step.
  const pendingUpdateRef = useRef<Update | null>(null);
  // Guard so we don't run concurrent check()/download() calls when both
  // the startup effect and the interval timer fire at the same instant.
  const runningRef = useRef(false);

  const guardAllowsUpdate = useCallback(async (): Promise<boolean> => {
    try {
      const guard = await invoke<BackendRecordingGuard>('check_for_update');
      return guard.canUpdate;
    } catch (err) {
      // If the guard command itself fails, play safe and skip.
      console.warn('[useUpdateCheck] guard command failed:', err);
      return false;
    }
  }, []);

  const runCheck = useCallback(async () => {
    if (runningRef.current) return;
    runningRef.current = true;
    try {
      // Respect the "don't update during recording" guard.
      if (!(await guardAllowsUpdate())) {
        return;
      }

      setStage('checking');
      setError(null);

      const update = await check();
      if (!update) {
        // Up to date. Reset to idle.
        setStage('idle');
        setAvailableVersion(null);
        pendingUpdateRef.current = null;
        return;
      }

      // New version available. Auto-download in the background.
      setAvailableVersion(update.version);
      setStage('available');
      pendingUpdateRef.current = update;

      setStage('downloading');
      await update.download((event) => {
        // Progress callback — we could surface bytes/total here, but for
        // v1 just transitioning the stage is enough to show activity.
        // Event shape: {event: 'Started' | 'Progress' | 'Finished', data?}
        if (event.event === 'Finished') {
          // Transition handled after await returns.
        }
      });

      setStage('ready');
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      console.error('[useUpdateCheck] failed:', msg);
      setError(msg);
      setStage('error');
    } finally {
      runningRef.current = false;
    }
  }, [guardAllowsUpdate]);

  const installNow = useCallback(async () => {
    if (!pendingUpdateRef.current) return;
    // Final guard: don't install mid-recording even if the download is
    // ready. User can click again when they stop recording.
    if (!(await guardAllowsUpdate())) {
      setError('Cannot install while a recording / upload is in progress.');
      return;
    }
    try {
      setStage('installing');
      // install() triggers the platform's native installer. On Windows
      // NSIS (passive mode) the installer runs in the background and
      // terminates the current process; on macOS the new .app replaces
      // the running one and Tauri auto-relaunches. Either way we don't
      // need to manually relaunch — the OS handles it.
      await pendingUpdateRef.current.install();
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      console.error('[useUpdateCheck] install failed:', msg);
      setError(msg);
      setStage('error');
    }
  }, [guardAllowsUpdate]);

  // Startup check + interval-based background checks.
  useEffect(() => {
    let cancelled = false;

    // Initial check shortly after mount (give the app a moment to finish
    // loading before we hit the network and the guard).
    const startupTimer = setTimeout(() => {
      if (!cancelled) void runCheck();
    }, 3000);

    // Recurring background checks. The interval adapts based on current
    // stage: slower when idle, faster when a download is ready (so the
    // "restart to update" nudge happens more often).
    const interval = setInterval(() => {
      if (cancelled) return;
      // Don't re-check if a download is already in flight.
      if (stage === 'checking' || stage === 'downloading' || stage === 'installing') {
        return;
      }
      void runCheck();
    }, stage === 'ready' ? READY_INTERVAL_MS : BACKGROUND_INTERVAL_MS);

    return () => {
      cancelled = true;
      clearTimeout(startupTimer);
      clearInterval(interval);
    };
  }, [runCheck, stage]);

  return {
    stage,
    availableVersion,
    error,
    checkNow: runCheck,
    installNow,
  };
}
