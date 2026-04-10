import { useRef, useEffect, useCallback, useImperativeHandle, forwardRef } from 'react';
import type { DrawTool, DrawColor, Point, StrokeAction } from './types';
import { drawStroke } from './drawUtils';

interface AnnotationCanvasProps {
  drawMode: boolean;
  tool: DrawTool;
  color: DrawColor;
  strokeWidth: number;
  onHistoryChange: (undoCount: number, redoCount: number) => void;
}

export interface AnnotationCanvasHandle {
  undo: () => void;
  redo: () => void;
  clear: () => void;
}

export const AnnotationCanvas = forwardRef<AnnotationCanvasHandle, AnnotationCanvasProps>(
  ({ drawMode, tool, color, strokeWidth, onHistoryChange }, ref) => {
    const historyCanvasRef = useRef<HTMLCanvasElement>(null);
    const activeCanvasRef = useRef<HTMLCanvasElement>(null);
    const actionsRef = useRef<StrokeAction[]>([]);
    const undoIndexRef = useRef(0);
    const currentStrokeRef = useRef<StrokeAction | null>(null);
    const isDrawingRef = useRef(false);

    const notifyHistory = useCallback(() => {
      onHistoryChange(undoIndexRef.current, actionsRef.current.length - undoIndexRef.current);
    }, [onHistoryChange]);

    const redrawHistory = useCallback(() => {
      const canvas = historyCanvasRef.current;
      if (!canvas) return;
      const ctx = canvas.getContext('2d');
      if (!ctx) return;
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      const dpr = window.devicePixelRatio || 1;
      ctx.save();
      ctx.scale(dpr, dpr);
      for (let i = 0; i < undoIndexRef.current; i++) {
        drawStroke(ctx, actionsRef.current[i]);
      }
      ctx.restore();
      notifyHistory();
    }, [notifyHistory]);

    const commitStroke = useCallback((action: StrokeAction) => {
      actionsRef.current = actionsRef.current.slice(0, undoIndexRef.current);
      actionsRef.current.push(action);
      undoIndexRef.current = actionsRef.current.length;
      redrawHistory();
    }, [redrawHistory]);

    useImperativeHandle(ref, () => ({
      undo() {
        if (undoIndexRef.current > 0) {
          undoIndexRef.current--;
          redrawHistory();
        }
      },
      redo() {
        if (undoIndexRef.current < actionsRef.current.length) {
          undoIndexRef.current++;
          redrawHistory();
        }
      },
      clear() {
        actionsRef.current = [];
        undoIndexRef.current = 0;
        redrawHistory();
      },
    }), [redrawHistory]);

    // Size canvases to window
    useEffect(() => {
      const resize = () => {
        const dpr = window.devicePixelRatio || 1;
        const w = window.innerWidth;
        const h = window.innerHeight;
        [historyCanvasRef, activeCanvasRef].forEach(canvasRef => {
          const canvas = canvasRef.current;
          if (!canvas) return;
          canvas.width = w * dpr;
          canvas.height = h * dpr;
          canvas.style.width = `${w}px`;
          canvas.style.height = `${h}px`;
        });
        redrawHistory();
      };
      resize();
      window.addEventListener('resize', resize);
      return () => window.removeEventListener('resize', resize);
    }, [redrawHistory]);

    // Keyboard shortcuts
    useEffect(() => {
      const handler = (e: KeyboardEvent) => {
        if (!drawMode) return;
        if (e.ctrlKey && e.key === 'z' && !e.shiftKey) {
          e.preventDefault();
          if (undoIndexRef.current > 0) {
            undoIndexRef.current--;
            redrawHistory();
          }
        }
        if (e.ctrlKey && (e.key === 'y' || (e.key === 'z' && e.shiftKey))) {
          e.preventDefault();
          if (undoIndexRef.current < actionsRef.current.length) {
            undoIndexRef.current++;
            redrawHistory();
          }
        }
      };
      window.addEventListener('keydown', handler);
      return () => window.removeEventListener('keydown', handler);
    }, [drawMode, redrawHistory]);

    const handleMouseDown = useCallback((e: React.MouseEvent) => {
      if (!drawMode || e.button !== 0) return;
      const point: Point = { x: e.clientX, y: e.clientY };
      currentStrokeRef.current = {
        tool,
        color,
        width: tool === 'highlighter' ? strokeWidth * 4 : strokeWidth,
        opacity: tool === 'highlighter' ? 0.35 : 1.0,
        points: [point],
      };
      isDrawingRef.current = true;
    }, [drawMode, tool, color, strokeWidth]);

    const handleMouseMove = useCallback((e: React.MouseEvent) => {
      if (!isDrawingRef.current || !currentStrokeRef.current) return;
      const point: Point = { x: e.clientX, y: e.clientY };

      if (currentStrokeRef.current.tool === 'pen' || currentStrokeRef.current.tool === 'highlighter') {
        currentStrokeRef.current.points.push(point);
      } else {
        currentStrokeRef.current.points = [currentStrokeRef.current.points[0], point];
      }

      const canvas = activeCanvasRef.current;
      if (!canvas) return;
      const ctx = canvas.getContext('2d');
      if (!ctx) return;
      const dpr = window.devicePixelRatio || 1;
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      ctx.save();
      ctx.scale(dpr, dpr);
      drawStroke(ctx, currentStrokeRef.current);
      ctx.restore();
    }, []);

    const handleMouseUp = useCallback(() => {
      if (currentStrokeRef.current && currentStrokeRef.current.points.length >= 2) {
        commitStroke(currentStrokeRef.current);
      }
      currentStrokeRef.current = null;
      isDrawingRef.current = false;
      const canvas = activeCanvasRef.current;
      if (canvas) {
        const ctx = canvas.getContext('2d');
        if (ctx) ctx.clearRect(0, 0, canvas.width, canvas.height);
      }
    }, [commitStroke]);

    return (
      <>
        <canvas
          ref={historyCanvasRef}
          style={{ position: 'fixed', inset: 0, pointerEvents: 'none', zIndex: 1 }}
        />
        <canvas
          ref={activeCanvasRef}
          onMouseDown={handleMouseDown}
          onMouseMove={handleMouseMove}
          onMouseUp={handleMouseUp}
          onMouseLeave={handleMouseUp}
          style={{
            position: 'fixed',
            inset: 0,
            zIndex: 2,
            cursor: drawMode ? 'crosshair' : 'default',
            pointerEvents: drawMode ? 'auto' : 'none',
          }}
        />
      </>
    );
  }
);

AnnotationCanvas.displayName = 'AnnotationCanvas';
