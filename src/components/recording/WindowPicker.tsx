import { useEffect, useState, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useRecordingStore } from '../../store/recordingStore';

interface WindowInfo {
  hwnd: number;
  title: string;
  x: number;
  y: number;
  width: number;
  height: number;
  thumbnail: string; // data:image/png;base64,...
}

const WindowIcon = () => (
  <svg className="w-6 h-6 text-slate-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
    <rect x="3" y="4" width="18" height="16" rx="2" />
    <path d="M3 8h18" />
    <circle cx="6" cy="6" r="0.75" fill="currentColor" />
    <circle cx="9" cy="6" r="0.75" fill="currentColor" />
  </svg>
);

export const WindowPicker = () => {
  const [windows, setWindows] = useState<WindowInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const { captureTarget, setCaptureTarget } = useRecordingStore();

  const fetchWindows = useCallback(() => {
    setLoading(true);
    invoke<WindowInfo[]>('list_windows')
      .then(setWindows)
      .catch((err) => console.error('[WindowPicker] list_windows failed:', err))
      .finally(() => setLoading(false));
  }, []);

  // Fetch on mount only. Do NOT auto-refresh on visibilitychange — that
  // would re-order the grid mid-interaction and the user could click a
  // card whose underlying window changed. Use the explicit Refresh button
  // when the list needs to update.
  useEffect(() => {
    fetchWindows();
  }, [fetchWindows]);

  const selectedHwnd =
    captureTarget.mode === 'window' ? captureTarget.hwnd : null;

  return (
    <div className="flex flex-col gap-2">
      {/* Header with refresh button */}
      <div className="flex items-center justify-between">
        <span className="text-[11px] text-slate-400 uppercase tracking-wide font-medium">
          {loading ? 'Loading windows...' : `${windows.length} window${windows.length !== 1 ? 's' : ''}`}
        </span>
        <button
          onClick={fetchWindows}
          disabled={loading}
          className="flex items-center gap-1 px-2 py-1 rounded-md text-[11px] text-slate-500 hover:text-slate-700 hover:bg-slate-100 transition disabled:opacity-40"
        >
          <svg
            className={`w-3 h-3 ${loading ? 'animate-spin' : ''}`}
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
            strokeWidth={2}
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"
            />
          </svg>
          Refresh
        </button>
      </div>

      {/* Scrollable grid */}
      <div className="overflow-y-auto" style={{ maxHeight: 280 }}>
        {loading && windows.length === 0 ? (
          <div className="flex items-center justify-center py-10">
            <div className="w-5 h-5 border-2 border-slate-200 border-t-slate-500 rounded-full animate-spin" />
          </div>
        ) : windows.length === 0 ? (
          <div className="py-8 text-center text-[12px] text-slate-400">No windows found</div>
        ) : (
          <div className="grid grid-cols-3 gap-2 pr-1">
            {windows.map((w) => {
              const isSelected = selectedHwnd === w.hwnd;
              return (
                <button
                  key={w.hwnd}
                  onClick={() => {
                    console.log('[WindowPicker] click', { hwnd: w.hwnd, title: w.title });
                    setCaptureTarget({
                      mode: 'window',
                      title: w.title,
                      hwnd: w.hwnd,
                      rect: { x: w.x, y: w.y, width: w.width, height: w.height },
                    });
                  }}
                  className={`flex flex-col rounded-lg border overflow-hidden text-left transition ${
                    isSelected
                      ? 'border-blue-500 ring-2 ring-blue-500/30'
                      : 'border-slate-200 hover:border-slate-300'
                  }`}
                >
                  {/* Thumbnail */}
                  <div className="w-full aspect-video bg-slate-100 flex items-center justify-center overflow-hidden">
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
                    <p className="text-[10px] text-slate-700 leading-tight line-clamp-1 font-medium">
                      {w.title || 'Untitled'}
                    </p>
                  </div>
                </button>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
};
