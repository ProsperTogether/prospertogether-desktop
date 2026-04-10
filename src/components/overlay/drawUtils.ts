import type { StrokeAction } from './types';

export function drawStroke(ctx: CanvasRenderingContext2D, action: StrokeAction) {
  ctx.save();
  ctx.globalAlpha = action.opacity;
  ctx.strokeStyle = action.color;
  ctx.lineWidth = action.width;
  ctx.lineCap = 'round';
  ctx.lineJoin = 'round';

  switch (action.tool) {
    case 'pen':
    case 'highlighter':
      if (action.points.length < 2) break;
      ctx.beginPath();
      ctx.moveTo(action.points[0].x, action.points[0].y);
      for (let i = 1; i < action.points.length; i++) {
        // Smooth curves using quadratic bezier between midpoints
        const prev = action.points[i - 1];
        const curr = action.points[i];
        const mx = (prev.x + curr.x) / 2;
        const my = (prev.y + curr.y) / 2;
        ctx.quadraticCurveTo(prev.x, prev.y, mx, my);
      }
      // Draw to last point
      const last = action.points[action.points.length - 1];
      ctx.lineTo(last.x, last.y);
      ctx.stroke();
      break;

    case 'rectangle':
      if (action.points.length < 2) break;
      const [r0, r1] = action.points;
      ctx.strokeRect(
        Math.min(r0.x, r1.x),
        Math.min(r0.y, r1.y),
        Math.abs(r1.x - r0.x),
        Math.abs(r1.y - r0.y),
      );
      break;

    case 'arrow': {
      if (action.points.length < 2) break;
      const [a0, a1] = action.points;
      // Shaft
      ctx.beginPath();
      ctx.moveTo(a0.x, a0.y);
      ctx.lineTo(a1.x, a1.y);
      ctx.stroke();
      // Arrowhead
      const angle = Math.atan2(a1.y - a0.y, a1.x - a0.x);
      const headLen = Math.max(action.width * 4, 14);
      ctx.beginPath();
      ctx.moveTo(a1.x, a1.y);
      ctx.lineTo(
        a1.x - headLen * Math.cos(angle - Math.PI / 6),
        a1.y - headLen * Math.sin(angle - Math.PI / 6),
      );
      ctx.moveTo(a1.x, a1.y);
      ctx.lineTo(
        a1.x - headLen * Math.cos(angle + Math.PI / 6),
        a1.y - headLen * Math.sin(angle + Math.PI / 6),
      );
      ctx.stroke();
      break;
    }
  }

  ctx.restore();
}
