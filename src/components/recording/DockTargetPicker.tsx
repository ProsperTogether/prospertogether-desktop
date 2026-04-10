import { useState, useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useRecordingStore } from '../../store/recordingStore';
import { launchRegionPicker } from '../../overlay/useRegionPicker';
import type { CaptureTarget } from '../../types/capture';

interface WindowInfo {
  hwnd: number;
  title: string;
  x: number;
  y: number;
  width: number;
  height: number;
  thumbnail: string;
}

interface MonitorInfo {
  id: number;
  name: string;
  x: number;
  y: number;
  width: number;
  height: number;
  primary: boolean;
}

interface DockTargetPickerProps {
  onSwitch: (target: CaptureTarget) => void;
  onClose: () => void;
}

type Section = 'window' | 'screen' | 'region';

const WindowIcon = () => (
  <svg className="w-5 h-5 text-slate-500" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
    <rect x="3" y="4" width="18" height="16" rx="2" />
    <path d="M3 8h18" />
    <circle cx="6" cy="6" r="0.75" fill="currentColor" />
    <circle cx="9" cy="6" r="0.75" fill="currentColor" />
  </svg>
);

export const DockTargetPicker = ({ onSwitch, onClose }: DockTargetPickerProps) => {
  const [section, setSection] = useState<Section>('window');
  const [windows, setWindows] = useState<WindowInfo[]>([]);
  const [monitors, setMonitors] = useState<MonitorInfo[]>([]);
  const [loadingWindows, setLoadingWindows] = useState(true);
  const [loadingMonitors, setLoadingMonitors] = useState(false);
  const panelRef = useRef<HTMLDivElement>(null);
  const { captureTarget } = useRecordingStore();

  // Close on outside click
  useEffect(() => {
    const handleMouseDown = (e: MouseEvent) => {
      if (panelRef.current && !panelRef.current.contains(e.target as Node)) {
        onClose();
      }
    };
    document.addEventListener('mousedown', handleMouseDown);
    return () => document.removeEventListener('mousedown', handleMouseDown);
  }, [onClose]);

  // Fetch windows on mount (default section)
  useEffect(() => {
    setLoadingWindows(true);
    invoke<WindowInfo[]>('list_windows')
      .then(setWindows)
      .catch((err) => console.error('[DockTargetPicker] list_windows failed:', err))
      .finally(() => setLoadingWindows(false));
  }, []);

  // Fetch monitors when screen section is opened
  useEffect(() => {
    if (section !== 'screen') return;
    setLoadingMonitors(true);
    invoke<MonitorInfo[]>('list_monitors')
      .then(setMonitors)
      .catch((err) => console.error('[DockTargetPicker] list_monitors failed:', err))
      .finally(() => setLoadingMonitors(false));
  }, [section]);

  const handleSelectWindow = (w: WindowInfo) => {
    onSwitch({
      mode: 'window',
      title: w.title,
      hwnd: w.hwnd,
      rect: { x: w.x, y: w.y, width: w.width, height: w.height },
    });
    onClose();
  };

  const handleSelectMonitor = (m: MonitorInfo, allMonitors: MonitorInfo[]) => {
    if (allMonitors.length <= 1) {
      onSwitch({ mode: 'screen' });
    } else {
      onSwitch({ mode: 'monitor', x: m.x, y: m.y, width: m.width, height: m.height });
    }
    onClose();
  };

  const handleSelectAllScreens = () => {
    onSwitch({ mode: 'screen' });
    onClose();
  };

  const handleSelectRegion = async () => {
    onClose();
    try {
      const rect = await launchRegionPicker();
      if (rect) {
        onSwitch({ mode: 'region', x: rect.x, y: rect.y, width: rect.width, height: rect.height });
      }
    } catch (err) {
      console.error('[DockTargetPicker] launchRegionPicker failed:', err);
    }
  };

  const selectedHwnd = captureTarget.mode === 'window' ? captureTarget.hwnd : null;

  return (
    <div
      ref={panelRef}
      className="absolute left-0 top-full mt-1 z-[10000] w-72 rounded-xl bg-slate-800/95 backdrop-blur border border-white/10 shadow-2xl shadow-black/50 overflow-hidden"
      onClick={(e) => e.stopPropagation()}
    >
      {/* Section tabs */}
      <div className="flex border-b border-white/10">
        {(['window', 'screen', 'region'] as Section[]).map((s) => (
          <button
            key={s}
            onClick={() => setSection(s)}
            className={`flex-1 px-2 py-2 text-[11px] font-medium transition ${
              section === s
                ? 'text-white bg-white/10'
                : 'text-slate-400 hover:text-white hover:bg-white/5'
            }`}
          >
            {s === 'window' ? 'Window' : s === 'screen' ? 'Screen' : 'Region'}
          </button>
        ))}
      </div>

      {/* Content */}
      <div className="overflow-y-auto" style={{ maxHeight: 240 }}>
        {/* Window section */}
        {section === 'window' && (
          <div className="p-2">
            {loadingWindows ? (
              <div className="flex items-center justify-center py-8">
                <div className="w-4 h-4 border-2 border-slate-600 border-t-slate-300 rounded-full animate-spin" />
              </div>
            ) : windows.length === 0 ? (
              <p className="py-6 text-center text-[11px] text-slate-500">No windows found</p>
            ) : (
              <div className="grid grid-cols-2 gap-1.5">
                {windows.map((w) => {
                  const isSelected = selectedHwnd === w.hwnd;
                  return (
                    <button
                      key={w.hwnd}
                      onClick={() => handleSelectWindow(w)}
                      className={`flex flex-col rounded-lg border overflow-hidden text-left transition ${
                        isSelected
                          ? 'border-blue-500/70 ring-1 ring-blue-500/40 bg-blue-500/10'
                          : 'border-white/10 hover:border-white/25 hover:bg-white/5'
                      }`}
                    >
                      {/* Thumbnail */}
                      <div className="w-full bg-slate-900 flex items-center justify-center overflow-hidden" style={{ height: 48 }}>
                        {w.thumbnail ? (
                          <img
                            src={w.thumbnail}
                            alt={w.title}
                            className="w-full h-full object-cover"
                            draggable={false}
                          />
                        ) : (
                          <WindowIcon />
                        )}
                      </div>
                      {/* Title */}
                      <div className="px-1.5 py-1">
                        <p className="text-[10px] text-slate-300 leading-tight truncate font-medium">
                          {w.title || 'Untitled'}
                        </p>
                      </div>
                    </button>
                  );
                })}
              </div>
            )}
          </div>
        )}

        {/* Screen section */}
        {section === 'screen' && (
          <div className="p-2 flex flex-col gap-1">
            {loadingMonitors ? (
              <div className="flex items-center justify-center py-8">
                <div className="w-4 h-4 border-2 border-slate-600 border-t-slate-300 rounded-full animate-spin" />
              </div>
            ) : (
              <>
                {/* All Screens option (shown when there are multiple monitors) */}
                {monitors.length > 1 && (
                  <button
                    onClick={handleSelectAllScreens}
                    className={`flex items-center gap-2 px-2.5 py-2 rounded-lg border text-left transition ${
                      captureTarget.mode === 'screen'
                        ? 'border-blue-500/70 bg-blue-500/10 text-white'
                        : 'border-white/10 hover:border-white/25 hover:bg-white/5 text-slate-300'
                    }`}
                  >
                    <svg className="w-4 h-4 text-slate-400 flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
                      <rect x="2" y="4" width="8" height="6" rx="1" />
                      <rect x="14" y="4" width="8" height="6" rx="1" />
                      <rect x="6" y="14" width="12" height="6" rx="1" />
                    </svg>
                    <div>
                      <p className="text-[12px] font-medium leading-tight">All Monitors</p>
                      <p className="text-[10px] text-slate-500">{monitors.length} screens</p>
                    </div>
                  </button>
                )}

                {/* Individual monitors */}
                {monitors.map((m, index) => {
                  const isSelected =
                    captureTarget.mode === 'monitor' &&
                    captureTarget.x === m.x &&
                    captureTarget.y === m.y;
                  return (
                    <button
                      key={m.id}
                      onClick={() => handleSelectMonitor(m, monitors)}
                      className={`flex items-center gap-2 px-2.5 py-2 rounded-lg border text-left transition ${
                        isSelected
                          ? 'border-blue-500/70 bg-blue-500/10 text-white'
                          : 'border-white/10 hover:border-white/25 hover:bg-white/5 text-slate-300'
                      }`}
                    >
                      <svg className="w-4 h-4 text-slate-400 flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
                        <rect x="2" y="3" width="20" height="14" rx="2" />
                        <path d="M8 21h8M12 17v4" />
                      </svg>
                      <div>
                        <p className="text-[12px] font-medium leading-tight">
                          Monitor {index + 1}
                          {m.primary && <span className="ml-1 text-[10px] text-blue-400 font-normal">(Primary)</span>}
                        </p>
                        <p className="text-[10px] text-slate-500">{m.width}×{m.height}</p>
                      </div>
                    </button>
                  );
                })}

                {/* Fallback for single monitor */}
                {monitors.length <= 1 && monitors.length > 0 && (
                  <button
                    onClick={handleSelectAllScreens}
                    className={`flex items-center gap-2 px-2.5 py-2 rounded-lg border text-left transition ${
                      captureTarget.mode === 'screen'
                        ? 'border-blue-500/70 bg-blue-500/10 text-white'
                        : 'border-white/10 hover:border-white/25 hover:bg-white/5 text-slate-300'
                    }`}
                  >
                    <svg className="w-4 h-4 text-slate-400 flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
                      <rect x="2" y="3" width="20" height="14" rx="2" />
                      <path d="M8 21h8M12 17v4" />
                    </svg>
                    <div>
                      <p className="text-[12px] font-medium leading-tight">Entire Screen</p>
                      <p className="text-[10px] text-slate-500">{monitors[0].width}×{monitors[0].height}</p>
                    </div>
                  </button>
                )}

                {monitors.length === 0 && (
                  <button
                    onClick={handleSelectAllScreens}
                    className="flex items-center gap-2 px-2.5 py-2 rounded-lg border border-white/10 hover:border-white/25 hover:bg-white/5 text-slate-300 text-left transition"
                  >
                    <svg className="w-4 h-4 text-slate-400 flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
                      <rect x="2" y="3" width="20" height="14" rx="2" />
                      <path d="M8 21h8M12 17v4" />
                    </svg>
                    <p className="text-[12px] font-medium">Entire Screen</p>
                  </button>
                )}
              </>
            )}
          </div>
        )}

        {/* Region section */}
        {section === 'region' && (
          <div className="p-3 flex flex-col items-center justify-center" style={{ minHeight: 80 }}>
            <p className="text-[11px] text-slate-400 mb-3 text-center">
              Draw a rectangle to select a capture region
            </p>
            <button
              onClick={handleSelectRegion}
              className="flex items-center gap-1.5 px-4 py-2 bg-blue-600 hover:bg-blue-500 text-white text-[12px] font-medium rounded-lg transition shadow-lg shadow-blue-600/25"
            >
              <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M4 8V6a2 2 0 012-2h2M4 16v2a2 2 0 002 2h2M16 4h2a2 2 0 012 2v2M16 20h2a2 2 0 002-2v-2" />
              </svg>
              Select Region
            </button>
          </div>
        )}
      </div>
    </div>
  );
};
