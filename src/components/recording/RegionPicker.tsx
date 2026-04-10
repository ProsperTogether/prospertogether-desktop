import { useRecordingStore } from '../../store/recordingStore';

const launchRegionPicker = () => {
  // TODO: Task 12 — launch the region-selection overlay window
  console.log('[RegionPicker] TODO: launch region picker overlay');
};

export const RegionPicker = () => {
  const { captureTarget, setCaptureTarget } = useRecordingStore();

  const hasRegion = captureTarget.mode === 'region';

  return (
    <div className="flex flex-col gap-2">
      {hasRegion ? (
        <div className="flex items-center justify-between px-3 py-2.5 rounded-lg border border-slate-200 bg-slate-50">
          <div>
            <p className="text-[12px] font-medium text-slate-800">Selected Region</p>
            <p className="text-[11px] text-slate-500 mt-0.5">
              {captureTarget.width}×{captureTarget.height} at ({captureTarget.x}, {captureTarget.y})
            </p>
          </div>
          <button
            onClick={launchRegionPicker}
            className="px-2.5 py-1 text-[11px] font-medium text-blue-600 hover:text-blue-700 hover:bg-blue-50 rounded-md transition"
          >
            Re-select
          </button>
        </div>
      ) : (
        <button
          onClick={launchRegionPicker}
          className="flex items-center justify-center gap-2 px-4 py-3 rounded-lg border-2 border-dashed border-slate-300 hover:border-blue-400 hover:bg-blue-50/50 text-slate-500 hover:text-blue-600 transition"
        >
          <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M4 8V6a2 2 0 012-2h2M4 16v2a2 2 0 002 2h2M16 4h2a2 2 0 012 2v2M16 20h2a2 2 0 002-2v-2" />
          </svg>
          <span className="text-[13px] font-medium">Select Region</span>
        </button>
      )}
    </div>
  );
};
