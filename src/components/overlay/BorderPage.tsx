import { useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow, LogicalPosition, LogicalSize } from '@tauri-apps/api/window';

const BORDER_WIDTH = 4;
const BORDER_COLOR = '#3b82f6'; // blue-500

export const BorderPage = () => {
  useEffect(() => {
    // Listen for reposition events from the window watcher
    let unlisten: (() => void) | undefined;

    listen<{ x: number; y: number; width: number; height: number }>('border:reposition', async (event) => {
      const win = getCurrentWindow();
      const { x, y, width, height } = event.payload;

      // Convert physical pixels to logical (border overlay needs logical)
      const sf = window.devicePixelRatio || 1;
      await win.setPosition(new LogicalPosition(
        Math.round(x / sf) - BORDER_WIDTH,
        Math.round(y / sf) - BORDER_WIDTH,
      ));
      await win.setSize(new LogicalSize(
        Math.round(width / sf) + BORDER_WIDTH * 2,
        Math.round(height / sf) + BORDER_WIDTH * 2,
      ));
    }).then(u => { unlisten = u; });

    return () => { unlisten?.(); };
  }, []);

  return (
    <div
      style={{
        position: 'fixed',
        inset: 0,
        border: `${BORDER_WIDTH}px solid ${BORDER_COLOR}`,
        borderRadius: '2px',
        pointerEvents: 'none',
        background: 'transparent',
      }}
    />
  );
};
