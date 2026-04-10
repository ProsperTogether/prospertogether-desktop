import { useUpdateCheck } from '../../hooks/useUpdateCheck';

/**
 * Small banner / status chip that surfaces the current auto-update state.
 * Hidden when the app is up-to-date or no check has run yet. Shows:
 *   - "Update available" while auto-downloading
 *   - "Downloading update…" with a spinner while fetching
 *   - "Restart to install v{version}" as a clickable button when ready
 *   - An error banner with the last failure message
 *
 * Mount this once somewhere that's visible on every screen (e.g., at the
 * top of the main layout or in the sidebar). It manages its own state
 * via the shared `useUpdateCheck` hook and won't double-trigger if
 * mounted more than once (the hook's internal `runningRef` guard).
 */
export const UpdateIndicator = () => {
  const { stage, availableVersion, error, installNow } = useUpdateCheck();

  if (stage === 'idle' || stage === 'checking') return null;

  if (stage === 'error') {
    return (
      <div className="fixed bottom-4 right-4 z-50 max-w-sm rounded-lg border border-amber-200 bg-amber-50 px-3.5 py-2.5 shadow-lg">
        <p className="text-[12px] font-medium text-amber-900">Update check failed</p>
        <p className="mt-0.5 text-[11px] text-amber-700 line-clamp-2">{error}</p>
      </div>
    );
  }

  if (stage === 'available' || stage === 'downloading') {
    return (
      <div className="fixed bottom-4 right-4 z-50 flex items-center gap-2 rounded-lg border border-blue-100 bg-blue-50 px-3.5 py-2.5 shadow-lg">
        <div className="h-3.5 w-3.5 animate-spin rounded-full border-2 border-blue-300 border-t-blue-600" />
        <div>
          <p className="text-[12px] font-medium text-blue-900">
            {stage === 'available' ? 'Update available' : 'Downloading update…'}
          </p>
          {availableVersion && (
            <p className="text-[11px] text-blue-700">v{availableVersion}</p>
          )}
        </div>
      </div>
    );
  }

  if (stage === 'ready') {
    return (
      <button
        onClick={installNow}
        className="fixed bottom-4 right-4 z-50 flex items-center gap-2 rounded-lg border border-green-200 bg-green-50 px-3.5 py-2.5 shadow-lg transition hover:bg-green-100"
      >
        <svg
          className="h-4 w-4 text-green-700"
          fill="none"
          viewBox="0 0 24 24"
          stroke="currentColor"
          strokeWidth={2}
        >
          <path strokeLinecap="round" strokeLinejoin="round" d="M12 4v16m8-8H4" />
        </svg>
        <div className="text-left">
          <p className="text-[12px] font-semibold text-green-900">
            Restart to update
          </p>
          {availableVersion && (
            <p className="text-[11px] text-green-700">v{availableVersion} ready</p>
          )}
        </div>
      </button>
    );
  }

  if (stage === 'installing') {
    return (
      <div className="fixed bottom-4 right-4 z-50 flex items-center gap-2 rounded-lg border border-slate-200 bg-white px-3.5 py-2.5 shadow-lg">
        <div className="h-3.5 w-3.5 animate-spin rounded-full border-2 border-slate-300 border-t-slate-600" />
        <p className="text-[12px] font-medium text-slate-900">Installing update…</p>
      </div>
    );
  }

  return null;
};
