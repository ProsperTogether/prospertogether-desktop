import { useEffect, useRef, useState } from 'react';
import { emit } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';

const MIN_SIZE = 100; // minimum selection size in logical pixels

interface SelectionRect {
  startX: number;
  startY: number;
  endX: number;
  endY: number;
}

function toDisplayRect(sel: SelectionRect) {
  return {
    left: Math.min(sel.startX, sel.endX),
    top: Math.min(sel.startY, sel.endY),
    width: Math.abs(sel.endX - sel.startX),
    height: Math.abs(sel.endY - sel.startY),
  };
}

export const RegionPickerPage = () => {
  const [selection, setSelection] = useState<SelectionRect | null>(null);
  const [dragging, setDragging] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const errorTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    const handleKeyDown = async (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        await emit('region:cancelled');
        const win = getCurrentWindow();
        await win.destroy();
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, []);

  // Clean up error timer on unmount
  useEffect(() => {
    return () => {
      if (errorTimerRef.current) clearTimeout(errorTimerRef.current);
    };
  }, []);

  const handleMouseDown = (e: React.MouseEvent) => {
    e.preventDefault();
    setError(null);
    setSelection({ startX: e.clientX, startY: e.clientY, endX: e.clientX, endY: e.clientY });
    setDragging(true);
  };

  const handleMouseMove = (e: React.MouseEvent) => {
    if (!dragging || !selection) return;
    setSelection(prev => prev ? { ...prev, endX: e.clientX, endY: e.clientY } : prev);
  };

  const handleMouseUp = async (e: React.MouseEvent) => {
    if (!dragging || !selection) return;
    setDragging(false);

    const finalSel = { ...selection, endX: e.clientX, endY: e.clientY };
    const { width, height } = toDisplayRect(finalSel);

    if (width < MIN_SIZE || height < MIN_SIZE) {
      setSelection(null);
      setError(`Selection too small. Minimum size is ${MIN_SIZE}×${MIN_SIZE} pixels.`);
      if (errorTimerRef.current) clearTimeout(errorTimerRef.current);
      errorTimerRef.current = setTimeout(() => setError(null), 2500);
      return;
    }

    // Convert to physical pixels by multiplying by devicePixelRatio
    const dpr = window.devicePixelRatio || 1;
    const physX = Math.round(Math.min(finalSel.startX, finalSel.endX) * dpr);
    const physY = Math.round(Math.min(finalSel.startY, finalSel.endY) * dpr);
    const physW = Math.round(width * dpr);
    const physH = Math.round(height * dpr);

    await emit('region:selected', { x: physX, y: physY, width: physW, height: physH });
    const win = getCurrentWindow();
    await win.destroy();
  };

  const displayRect = selection ? toDisplayRect(selection) : null;

  return (
    <div
      style={{
        position: 'fixed',
        inset: 0,
        background: 'rgba(0,0,0,0.3)',
        cursor: 'crosshair',
        userSelect: 'none',
      }}
      onMouseDown={handleMouseDown}
      onMouseMove={handleMouseMove}
      onMouseUp={handleMouseUp}
    >
      {/* Instructions */}
      <div
        style={{
          position: 'absolute',
          top: '50%',
          left: '50%',
          transform: 'translate(-50%, -50%)',
          color: 'white',
          fontSize: '15px',
          fontWeight: 500,
          textShadow: '0 1px 3px rgba(0,0,0,0.8)',
          pointerEvents: 'none',
          textAlign: 'center',
          // Hide instructions when actively dragging to avoid visual clutter
          opacity: dragging ? 0 : 1,
          transition: 'opacity 0.1s',
        }}
      >
        Click and drag to select a region. Press Escape to cancel.
      </div>

      {/* Selection rectangle */}
      {dragging && displayRect && displayRect.width > 0 && displayRect.height > 0 && (
        <div
          style={{
            position: 'absolute',
            left: displayRect.left,
            top: displayRect.top,
            width: displayRect.width,
            height: displayRect.height,
            border: '2px solid #3b82f6',
            background: 'rgba(59, 130, 246, 0.1)',
            pointerEvents: 'none',
          }}
        />
      )}

      {/* Error message */}
      {error && (
        <div
          style={{
            position: 'absolute',
            bottom: '48px',
            left: '50%',
            transform: 'translateX(-50%)',
            background: 'rgba(239, 68, 68, 0.9)',
            color: 'white',
            padding: '8px 16px',
            borderRadius: '6px',
            fontSize: '13px',
            fontWeight: 500,
            pointerEvents: 'none',
            whiteSpace: 'nowrap',
          }}
        >
          {error}
        </div>
      )}
    </div>
  );
};
