import { WebviewWindow } from '@tauri-apps/api/webviewWindow';
import { currentMonitor } from '@tauri-apps/api/window';
import type { Rect } from '../types/capture';

const BORDER_WIDTH = 4;

export async function createBorderOverlay(rect: Rect): Promise<WebviewWindow> {
  // Convert rect to logical pixels using scale factor
  const monitor = await currentMonitor();
  const sf = monitor?.scaleFactor || 1;

  // rect is in physical pixels (from gdigrab/Windows API), convert to logical
  const x = Math.round(rect.x / sf) - BORDER_WIDTH;
  const y = Math.round(rect.y / sf) - BORDER_WIDTH;
  const width = Math.round(rect.width / sf) + BORDER_WIDTH * 2;
  const height = Math.round(rect.height / sf) + BORDER_WIDTH * 2;

  const overlay = new WebviewWindow('border-overlay', {
    url: '/#/border',
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

  return new Promise((resolve, reject) => {
    overlay.once('tauri://created', async () => {
      // Make click-through
      try {
        await overlay.setIgnoreCursorEvents(true);
      } catch (err) {
        console.warn('[border-overlay] failed to set ignore cursor events:', err);
      }
      // Re-assert main window on top
      try {
        const main = await WebviewWindow.getByLabel('main');
        if (main) {
          await main.setAlwaysOnTop(true);
        }
      } catch (err) {
        console.warn('[border-overlay] failed to bump main window:', err);
      }
      resolve(overlay);
    });
    overlay.once('tauri://error', (e) => reject(e));
  });
}

export async function destroyBorderOverlay(): Promise<void> {
  const overlay = await WebviewWindow.getByLabel('border-overlay');
  if (overlay) {
    await overlay.destroy();
  }
}
