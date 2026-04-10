import { useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';

import { AnnotationCanvas, type AnnotationCanvasHandle } from './AnnotationCanvas';
import type { DrawTool, DrawColor } from './types';

/**
 * The overlay window is now a thin click-receiving canvas. The drawing tools
 * (pen/highlighter/arrow/rect, color picker, stroke width, undo/redo/clear)
 * live in the recording dock (main window) and push state to the overlay via
 * Tauri events. The canvas is permanently in active draw mode while this
 * window exists — the overlay's existence == drawing being on.
 */
export function OverlayPage() {
  const [tool, setTool] = useState<DrawTool>('pen');
  const [color, setColor] = useState<DrawColor>('#ef4444');
  const [strokeWidth, setStrokeWidth] = useState(4);
  const canvasRef = useRef<AnnotationCanvasHandle>(null);

  // Make background transparent
  useEffect(() => {
    document.documentElement.style.background = 'transparent';
    document.body.style.background = 'transparent';
    return () => {
      document.documentElement.style.background = '';
      document.body.style.background = '';
    };
  }, []);

  // Listen for tool/color/width changes + undo/redo/clear actions from the dock
  useEffect(() => {
    const unlisteners: Array<() => void> = [];

    listen<DrawTool>('overlay:set-tool', (e) => setTool(e.payload))
      .then(u => unlisteners.push(u));

    listen<DrawColor>('overlay:set-color', (e) => setColor(e.payload))
      .then(u => unlisteners.push(u));

    listen<number>('overlay:set-stroke-width', (e) => setStrokeWidth(e.payload))
      .then(u => unlisteners.push(u));

    listen('overlay:undo', () => canvasRef.current?.undo())
      .then(u => unlisteners.push(u));

    listen('overlay:redo', () => canvasRef.current?.redo())
      .then(u => unlisteners.push(u));

    listen('overlay:clear', () => canvasRef.current?.clear())
      .then(u => unlisteners.push(u));

    // Note: the window-follow reposition listener lives in the main
    // window (`useOverlayWindow.ts`), NOT here. The main window listens
    // for `border:reposition` and calls `overlayWin.setPosition/setSize`
    // from its own context so the logs are visible in main devtools.

    return () => { unlisteners.forEach(u => u()); };
  }, []);

  return (
    <div style={{ width: '100vw', height: '100vh', overflow: 'hidden', background: 'transparent' }}>
      <AnnotationCanvas
        ref={canvasRef}
        drawMode={true}
        tool={tool}
        color={color}
        strokeWidth={strokeWidth}
        onHistoryChange={() => { /* dock owns its own history counts; nothing to report back */ }}
      />
    </div>
  );
}
