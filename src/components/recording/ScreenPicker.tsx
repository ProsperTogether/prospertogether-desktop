import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useRecordingStore } from '../../store/recordingStore';

interface MonitorInfo {
  id: number;
  name: string;
  x: number;
  y: number;
  width: number;
  height: number;
  primary: boolean;
}

export const ScreenPicker = () => {
  const [monitors, setMonitors] = useState<MonitorInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const { captureTarget, setCaptureTarget } = useRecordingStore();

  useEffect(() => {
    invoke<MonitorInfo[]>('list_monitors')
      .then(setMonitors)
      .catch((err) => console.error('[ScreenPicker] list_monitors failed:', err))
      .finally(() => setLoading(false));
  }, []);

  if (loading) {
    return (
      <div className="py-2 text-[12px] text-slate-400">Detecting monitors...</div>
    );
  }

  // Single monitor — no selection needed
  if (monitors.length <= 1) {
    return (
      <div className="py-1 text-[12px] text-slate-500">
        Entire screen will be captured.
      </div>
    );
  }

  const isAllSelected = captureTarget.mode === 'screen';
  const selectedMonitorId =
    captureTarget.mode === 'monitor'
      ? monitors.find(
          (m) =>
            captureTarget.mode === 'monitor' &&
            m.x === captureTarget.x &&
            m.y === captureTarget.y &&
            m.width === captureTarget.width &&
            m.height === captureTarget.height
        )?.id ?? null
      : null;

  return (
    <div className="flex flex-col gap-2">
      {/* All Monitors option */}
      <button
        onClick={() => setCaptureTarget({ mode: 'screen' })}
        className={`flex items-center gap-3 px-3 py-2 rounded-lg border text-left transition ${
          isAllSelected
            ? 'border-blue-500 bg-blue-50 ring-2 ring-blue-500/30'
            : 'border-slate-200 hover:border-slate-300 hover:bg-slate-50'
        }`}
      >
        <div className="flex-shrink-0 w-8 h-6 rounded bg-slate-200 flex items-center justify-center">
          <svg className="w-4 h-4 text-slate-500" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
            <rect x="2" y="4" width="8" height="6" rx="1" />
            <rect x="14" y="4" width="8" height="6" rx="1" />
            <rect x="6" y="14" width="12" height="6" rx="1" />
          </svg>
        </div>
        <div>
          <p className="text-[13px] font-medium text-slate-800">All Monitors</p>
          <p className="text-[11px] text-slate-400">{monitors.length} screens combined</p>
        </div>
      </button>

      {/* Individual monitor cards */}
      <div className="grid grid-cols-2 gap-2">
        {monitors.map((m, index) => {
          const isSelected = selectedMonitorId === m.id;
          return (
            <button
              key={m.id}
              onClick={() =>
                setCaptureTarget({
                  mode: 'monitor',
                  x: m.x,
                  y: m.y,
                  width: m.width,
                  height: m.height,
                })
              }
              className={`flex flex-col gap-1 px-3 py-2.5 rounded-lg border text-left transition ${
                isSelected
                  ? 'border-blue-500 bg-blue-50 ring-2 ring-blue-500/30'
                  : 'border-slate-200 hover:border-slate-300 hover:bg-slate-50'
              }`}
            >
              {/* Mini monitor icon */}
              <div className="w-full h-8 rounded bg-slate-100 border border-slate-200 flex items-center justify-center mb-0.5">
                <div className="w-5 h-4 rounded-sm border border-slate-300 bg-slate-50" />
              </div>
              <p className="text-[12px] font-medium text-slate-800 leading-tight">
                Monitor {index + 1}
                {m.primary && (
                  <span className="ml-1 text-[10px] text-blue-500 font-normal">(Primary)</span>
                )}
              </p>
              <p className="text-[11px] text-slate-400">
                {m.width}×{m.height}
              </p>
            </button>
          );
        })}
      </div>
    </div>
  );
};
