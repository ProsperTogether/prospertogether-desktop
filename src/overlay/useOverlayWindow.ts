import { WebviewWindow } from '@tauri-apps/api/webviewWindow';
import { currentMonitor, LogicalPosition, LogicalSize } from '@tauri-apps/api/window';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import type { Rect } from '../types/capture';

/** Active reposition listener, one per drawing-overlay lifetime. */
let activeRepositionUnlisten: UnlistenFn | null = null;

/** Route a diagnostic message to Rust stderr so we can see it in the terminal. */
function dlog(message: string): void {
  // Fire-and-forget — don't slow down the reposition hot path.
  invoke('debug_log', { message }).catch(() => {});
}

// Vertical reserve at the top of the active monitor that the overlay window
// must NOT extend over. This corresponds to the recording dock (84px tall) +
// a small breathing margin so the user can always click the dock without
// accidentally drawing into it. Keep in sync with DOCK_HEIGHT in
// RecordingControls.tsx.
const DOCK_RESERVED_TOP_PX = 84 + 16;

/**
 * Create the annotation overlay window. The overlay is an always-on-top,
 * transparent window that captures pointer events for drawing.
 *
 * IMPORTANT: this is only called when the user explicitly enables drawing mode
 * from the recording dock — never on recording start.
 *
 * Geometry: the overlay covers the active monitor BELOW the dock area. We
 * deliberately do NOT cover the top strip where the dock lives, so clicks on
 * the dock buttons (Stop Drawing, Stop Recording, color picker, etc.) hit the
 * dock's HWND directly instead of being intercepted by the overlay. This
 * sidesteps the entire "always-on-top z-order on Windows is unreliable"
 * problem — z-order doesn't matter when the dock isn't even inside the
 * overlay's hit region.
 *
 * Multi-monitor: only the active monitor is covered. Drawing on other monitors
 * is not supported in this layout. We can revisit later if needed by spawning
 * additional overlay windows for the non-active monitors.
 */
export async function createOverlayWindow(captureRect?: Rect): Promise<WebviewWindow> {
  const monitor = await currentMonitor();
  if (!monitor) {
    throw new Error('No active monitor available for overlay');
  }

  // Tauri's monitor.position/size are in PHYSICAL pixels; WebviewWindow
  // x/y/width/height options use LOGICAL pixels. Divide by scale factor.
  const sf = monitor.scaleFactor || 1;

  let x: number, y: number, width: number, height: number;

  if (captureRect) {
    // Convert physical pixels to logical
    const rectXLogical = captureRect.x / sf;
    const rectYLogical = captureRect.y / sf;
    const rectWLogical = captureRect.width / sf;
    const rectHLogical = captureRect.height / sf;

    // If capture rect top is within dock area, shift overlay down
    const monitorYLogical = monitor.position.y / sf;
    const dockBottom = monitorYLogical + DOCK_RESERVED_TOP_PX;

    if (rectYLogical < dockBottom) {
      y = Math.round(dockBottom);
      height = Math.round(rectHLogical - (dockBottom - rectYLogical));
    } else {
      y = Math.round(rectYLogical);
      height = Math.round(rectHLogical);
    }
    x = Math.round(rectXLogical);
    width = Math.round(rectWLogical);
  } else {
    // Full monitor minus dock (existing behavior)
    const monitorXLogical = monitor.position.x / sf;
    const monitorYLogical = monitor.position.y / sf;
    const monitorWidthLogical = monitor.size.width / sf;
    const monitorHeightLogical = monitor.size.height / sf;

    x = Math.round(monitorXLogical);
    y = Math.round(monitorYLogical + DOCK_RESERVED_TOP_PX);
    width = Math.round(monitorWidthLogical);
    height = Math.round(monitorHeightLogical - DOCK_RESERVED_TOP_PX);
  }

  const overlay = new WebviewWindow('overlay', {
    url: '/#/overlay',
    transparent: true,
    decorations: false,
    alwaysOnTop: true,
    skipTaskbar: true,
    resizable: false,
    focus: false,
    shadow: false,
    x,
    y,
    width,
    height,
  });

  // Register the reposition listener IMMEDIATELY (before tauri://created)
  // so we can't miss the race where the watcher already started firing
  // before the create callback runs. The handler safely no-ops if the
  // overlay window doesn't exist yet.
  dlog(`createOverlayWindow ENTRY: captureRect=${JSON.stringify(captureRect)}`);
  if (captureRect !== undefined) {
    // Clean up any stale listener from a previous overlay lifetime.
    if (activeRepositionUnlisten) {
      try { activeRepositionUnlisten(); } catch {}
      activeRepositionUnlisten = null;
    }
    let firstEventLogged = false;
    let firstSetPosLogged = false;
    let errorLogged = false;
    try {
      activeRepositionUnlisten = await listen<{
        x: number;
        y: number;
        width: number;
        height: number;
      }>('border:reposition', async (ev) => {
        if (!firstEventLogged) {
          firstEventLogged = true;
          dlog(`FIRST border:reposition received ${JSON.stringify(ev.payload)}`);
        }
        try {
          const overlayWin = await WebviewWindow.getByLabel('overlay');
          if (!overlayWin) {
            // Overlay not yet created (or already destroyed) — skip.
            return;
          }
          const monitor = await currentMonitor();
          const sf = monitor?.scaleFactor || 1;
          let xL = ev.payload.x / sf;
          let yL = ev.payload.y / sf;
          let wL = ev.payload.width / sf;
          let hL = ev.payload.height / sf;

          // Clip top to stay below the dock (same as create-time logic).
          const monitorYLogical = (monitor?.position.y ?? 0) / sf;
          const dockBottom = monitorYLogical + DOCK_RESERVED_TOP_PX;
          if (yL < dockBottom) {
            const clip = dockBottom - yL;
            yL = dockBottom;
            hL = Math.max(0, hL - clip);
          }

          if (wL > 0 && hL > 0) {
            await overlayWin.setPosition(
              new LogicalPosition(Math.round(xL), Math.round(yL))
            );
            await overlayWin.setSize(
              new LogicalSize(Math.round(wL), Math.round(hL))
            );
            if (!firstSetPosLogged) {
              firstSetPosLogged = true;
              dlog(`FIRST overlay setPosition succeeded: ${Math.round(xL)},${Math.round(yL)} ${Math.round(wL)}x${Math.round(hL)}`);
            }
          }
        } catch (err) {
          if (!errorLogged) {
            errorLogged = true;
            dlog(`reposition handler error (once): ${err}`);
          }
        }
      });
      dlog('reposition listener registered (early, before tauri://created)');
    } catch (err) {
      dlog(`failed to register reposition listener: ${err}`);
    }
  } else {
    dlog('createOverlayWindow called without captureRect — listener NOT registered');
  }

  return new Promise((resolve, reject) => {
    overlay.once('tauri://created', async () => {
      dlog('overlay tauri://created fired');
      // Re-assert alwaysOnTop on the main (dock) window so it floats above
      // any other always-on-top sibling.
      try {
        const main = await WebviewWindow.getByLabel('main');
        if (main) {
          await main.setAlwaysOnTop(true);
          await main.setFocus();
        }
      } catch (err) {
        console.warn('[overlay] failed to bump main window above overlay:', err);
      }
      resolve(overlay);
    });
    overlay.once('tauri://error', (e) => {
      dlog(`overlay tauri://error: ${JSON.stringify(e)}`);
      reject(e);
    });
  });
}

export async function destroyOverlayWindow(): Promise<void> {
  // Unregister the reposition listener before destroying the window.
  if (activeRepositionUnlisten) {
    try { activeRepositionUnlisten(); } catch {}
    activeRepositionUnlisten = null;
  }
  const overlay = await WebviewWindow.getByLabel('overlay');
  if (overlay) {
    await overlay.destroy();
  }
}
