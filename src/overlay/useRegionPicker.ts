import { WebviewWindow } from '@tauri-apps/api/webviewWindow';
import { currentMonitor } from '@tauri-apps/api/window';
import { listen } from '@tauri-apps/api/event';
import type { Rect } from '../types/capture';

export async function launchRegionPicker(): Promise<Rect | null> {
  const monitor = await currentMonitor();
  if (!monitor) throw new Error('No monitor');

  const sf = monitor.scaleFactor || 1;
  const x = Math.round(monitor.position.x / sf);
  const y = Math.round(monitor.position.y / sf);
  const width = Math.round(monitor.size.width / sf);
  const height = Math.round(monitor.size.height / sf);

  // Hide main window
  const main = await WebviewWindow.getByLabel('main');
  if (main) await main.hide();

  const picker = new WebviewWindow('region-picker', {
    url: '/#/region-picker',
    transparent: true,
    decorations: false,
    alwaysOnTop: true,
    skipTaskbar: true,
    resizable: false,
    focus: true,
    shadow: false,
    x,
    y,
    width,
    height,
  });

  return new Promise<Rect | null>((resolve) => {
    let resolved = false;

    const cleanup = async () => {
      if (resolved) return;
      resolved = true;
      // Show main window
      const main = await WebviewWindow.getByLabel('main');
      if (main) {
        await main.show();
        await main.setFocus();
      }
    };

    listen<Rect>('region:selected', async (event) => {
      await cleanup();
      resolve(event.payload);
    });

    listen('region:cancelled', async () => {
      await cleanup();
      resolve(null);
    });

    picker.once('tauri://error', async () => {
      await cleanup();
      resolve(null);
    });
  });
}
