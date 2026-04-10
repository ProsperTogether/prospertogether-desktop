import { WebviewWindow } from '@tauri-apps/api/webviewWindow';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { listen } from '@tauri-apps/api/event';
import { launchRegionPicker } from './useRegionPicker';
import type { CaptureTarget } from '../types/capture';

const PICKER_WIDTH = 320;
const PICKER_HEIGHT = 340;

/**
 * Open the dock capture-target picker as a separate always-on-top window
 * positioned right below the main (dock) window. Returns when the user
 * selects a target or cancels.
 */
export async function openDockPicker(): Promise<CaptureTarget | null> {
  // Compute position: anchored to the left side of the dock, just below it
  const main = getCurrentWindow();
  const pos = await main.outerPosition();
  const size = await main.outerSize();
  const sf = (await main.scaleFactor()) || 1;

  // Convert physical to logical for window positioning
  const dockXLogical = pos.x / sf;
  const dockYLogical = pos.y / sf;
  const dockWidthLogical = size.width / sf;
  const dockHeightLogical = size.height / sf;

  // Anchor the picker to the left side of the dock (where the target indicator sits)
  const pickerX = Math.round(dockXLogical + 100); // offset from left edge
  const pickerY = Math.round(dockYLogical + dockHeightLogical + 4);

  // Destroy any existing picker window first
  const existing = await WebviewWindow.getByLabel('dock-picker');
  if (existing) await existing.destroy();

  const picker = new WebviewWindow('dock-picker', {
    url: '/#/dock-picker',
    width: PICKER_WIDTH,
    height: PICKER_HEIGHT,
    x: pickerX,
    y: pickerY,
    decorations: false,
    transparent: true,
    alwaysOnTop: true,
    skipTaskbar: true,
    resizable: false,
    focus: true,
    shadow: true,
  });

  return new Promise<CaptureTarget | null>((resolve) => {
    let resolved = false;
    let unlistenSwitch: (() => void) | undefined;
    let unlistenClose: (() => void) | undefined;
    let unlistenRegion: (() => void) | undefined;

    const destroyPicker = async () => {
      try {
        const p = await WebviewWindow.getByLabel('dock-picker');
        if (p) { try { await p.destroy(); } catch {} }
      } catch {}
    };

    const cleanup = async (result: CaptureTarget | null) => {
      if (resolved) return;
      resolved = true;
      unlistenSwitch?.();
      unlistenClose?.();
      unlistenRegion?.();
      await destroyPicker();
      resolve(result);
    };

    listen<CaptureTarget>('dock-picker:switch', async (event) => {
      await cleanup(event.payload);
    }).then(u => { unlistenSwitch = u; });

    listen('dock-picker:close', async () => {
      await cleanup(null);
    }).then(u => { unlistenClose = u; });

    // The picker window requests a region — we handle it from the main window
    // because the picker window itself gets destroyed. The picker emits this
    // event instead of closing normally, then we launch the region picker and
    // resolve with the resulting rect (or null if cancelled).
    listen('dock-picker:request-region', async () => {
      if (resolved) return;
      // Destroy the picker window first so it doesn't occlude the region picker
      await destroyPicker();
      const rect = await launchRegionPicker();
      if (rect) {
        resolved = true;
        unlistenSwitch?.();
        unlistenClose?.();
        unlistenRegion?.();
        resolve({ mode: 'region', x: rect.x, y: rect.y, width: rect.width, height: rect.height });
      } else {
        await cleanup(null);
      }
    }).then(u => { unlistenRegion = u; });

    // Also cleanup if the window itself errors out
    picker.once('tauri://error', async () => {
      await cleanup(null);
    });
  });
}
