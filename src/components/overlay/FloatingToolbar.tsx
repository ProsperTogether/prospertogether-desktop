import type { DrawTool, DrawColor } from './types';

const COLORS: DrawColor[] = [
  '#ef4444', // red
  '#f97316', // orange
  '#eab308', // yellow
  '#22c55e', // green
  '#3b82f6', // blue
  '#ffffff', // white
];

const WIDTHS = [2, 4, 8];

interface FloatingToolbarProps {
  tool: DrawTool;
  color: DrawColor;
  strokeWidth: number;
  drawingActive: boolean;
  onToolChange: (tool: DrawTool) => void;
  onColorChange: (color: DrawColor) => void;
  onStrokeWidthChange: (width: number) => void;
  onUndo: () => void;
  onRedo: () => void;
  onClear: () => void;
  onToggleDrawing: () => void;
}

const ToolButton = ({
  active,
  disabled,
  onClick,
  title,
  children,
}: {
  active?: boolean;
  disabled?: boolean;
  onClick: () => void;
  title: string;
  children: React.ReactNode;
}) => (
  <button
    onClick={onClick}
    disabled={disabled}
    title={title}
    style={{
      width: 36,
      height: 36,
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      borderRadius: 8,
      border: 'none',
      cursor: disabled ? 'not-allowed' : 'pointer',
      background: active ? 'rgba(59,130,246,0.3)' : 'transparent',
      color: disabled ? '#475569' : active ? '#60a5fa' : '#cbd5e1',
      transition: 'all 0.15s',
      opacity: disabled ? 0.4 : 1,
      padding: 0,
    }}
    onMouseEnter={(e) => {
      if (!active && !disabled) e.currentTarget.style.background = 'rgba(255,255,255,0.08)';
    }}
    onMouseLeave={(e) => {
      if (!active && !disabled) e.currentTarget.style.background = 'transparent';
    }}
  >
    {children}
  </button>
);

const Divider = () => (
  <div style={{ width: 1, height: 24, background: 'rgba(255,255,255,0.1)', margin: '0 4px' }} />
);

/**
 * Horizontal drawing toolbar — pen / highlighter / arrow / rect, color picker,
 * stroke width, undo/redo/clear, plus a Start/Stop Drawing toggle.
 *
 * The "Start Drawing" button is the gating control: until the user presses it,
 * no annotation overlay window exists at all and the desktop is fully clickable.
 * Once pressed, the overlay window appears and pointer events are captured by
 * the canvas for drawing.
 *
 * Designed to be embedded inside the recording dock at the top of the screen.
 */
export function FloatingToolbar({
  tool,
  color,
  strokeWidth,
  drawingActive,
  onToolChange,
  onColorChange,
  onStrokeWidthChange,
  onUndo,
  onRedo,
  onClear,
  onToggleDrawing,
}: FloatingToolbarProps) {
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'row',
        alignItems: 'center',
        gap: 2,
        background: 'rgba(15, 23, 42, 0.65)',
        backdropFilter: 'blur(12px)',
        borderRadius: 12,
        padding: '4px 6px',
        border: '1px solid rgba(255,255,255,0.08)',
        boxShadow: '0 4px 16px rgba(0,0,0,0.3)',
        userSelect: 'none',
      }}
    >
      {/* Tools */}
      <ToolButton active={tool === 'pen'} onClick={() => onToolChange('pen')} title="Pen (P)">
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M17 3a2.828 2.828 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5L17 3z" />
        </svg>
      </ToolButton>
      <ToolButton active={tool === 'highlighter'} onClick={() => onToolChange('highlighter')} title="Highlighter (H)">
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M9 11l-6 6v3h9l3-3" />
          <path d="M22 12l-4.6 4.6a2 2 0 01-2.8 0l-5.2-5.2a2 2 0 010-2.8L14 4" />
        </svg>
      </ToolButton>
      <ToolButton active={tool === 'arrow'} onClick={() => onToolChange('arrow')} title="Arrow (A)">
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <line x1="5" y1="19" x2="19" y2="5" />
          <polyline points="11 5 19 5 19 13" />
        </svg>
      </ToolButton>
      <ToolButton active={tool === 'rectangle'} onClick={() => onToolChange('rectangle')} title="Rectangle (R)">
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <rect x="3" y="3" width="18" height="18" rx="2" />
        </svg>
      </ToolButton>

      <Divider />

      {/* Colors */}
      <div style={{ display: 'flex', gap: 5, padding: '0 4px' }}>
        {COLORS.map((c) => (
          <button
            key={c}
            onClick={() => onColorChange(c)}
            title={c}
            style={{
              width: 18,
              height: 18,
              borderRadius: '50%',
              border: color === c ? '2px solid #60a5fa' : '2px solid rgba(255,255,255,0.18)',
              background: c,
              cursor: 'pointer',
              padding: 0,
              outline: 'none',
              boxShadow: color === c ? '0 0 0 2px rgba(96,165,250,0.25)' : 'none',
              transition: 'all 0.15s',
            }}
          />
        ))}
      </div>

      <Divider />

      {/* Stroke width */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 2 }}>
        {WIDTHS.map((w) => (
          <button
            key={w}
            onClick={() => onStrokeWidthChange(w)}
            title={`Size ${w}`}
            style={{
              width: 30,
              height: 30,
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              borderRadius: 6,
              border: 'none',
              background: strokeWidth === w ? 'rgba(59,130,246,0.3)' : 'transparent',
              cursor: 'pointer',
              padding: 0,
              transition: 'all 0.15s',
            }}
          >
            <div
              style={{
                width: 18,
                height: Math.max(w, 2),
                borderRadius: w,
                background: strokeWidth === w ? '#60a5fa' : '#94a3b8',
              }}
            />
          </button>
        ))}
      </div>

      <Divider />

      {/* Undo / Redo / Clear */}
      <ToolButton onClick={onUndo} disabled={!drawingActive} title="Undo (Ctrl+Z)">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <polyline points="1 4 1 10 7 10" />
          <path d="M3.51 15a9 9 0 1 0 2.13-9.36L1 10" />
        </svg>
      </ToolButton>
      <ToolButton onClick={onRedo} disabled={!drawingActive} title="Redo (Ctrl+Y)">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <polyline points="23 4 23 10 17 10" />
          <path d="M20.49 15a9 9 0 1 1-2.13-9.36L23 10" />
        </svg>
      </ToolButton>
      <ToolButton onClick={onClear} disabled={!drawingActive} title="Clear all">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <polyline points="3 6 5 6 21 6" />
          <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
        </svg>
      </ToolButton>

      <Divider />

      {/* Start / Stop Drawing toggle */}
      <button
        onClick={onToggleDrawing}
        title={drawingActive ? 'Stop drawing (canvas captures clicks)' : 'Start drawing (canvas captures clicks)'}
        style={{
          height: 30,
          padding: '0 12px',
          marginLeft: 2,
          marginRight: 2,
          borderRadius: 6,
          border: drawingActive ? '1px solid rgba(96,165,250,0.5)' : '1px solid rgba(255,255,255,0.15)',
          background: drawingActive ? 'rgba(59,130,246,0.3)' : 'rgba(255,255,255,0.05)',
          color: drawingActive ? '#93c5fd' : '#cbd5e1',
          fontSize: 12,
          fontWeight: 600,
          cursor: 'pointer',
          transition: 'all 0.15s',
          display: 'flex',
          alignItems: 'center',
          gap: 6,
        }}
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M17 3a2.828 2.828 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5L17 3z" />
        </svg>
        {drawingActive ? 'Stop Drawing' : 'Start Drawing'}
      </button>
    </div>
  );
}
