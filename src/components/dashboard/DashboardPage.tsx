import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';

import { listMySubmissions, type AgentSubmission } from '../../api/submissions';

const COLUMN_LABELS: Record<string, string> = {
  backlog: 'Backlog',
  in_progress: 'In Progress',
  review: 'Review',
  done: 'Done',
};

const COLUMN_DOTS: Record<string, string> = {
  backlog: 'bg-slate-400',
  in_progress: 'bg-blue-500',
  review: 'bg-amber-500',
  done: 'bg-emerald-500',
};

export const DashboardPage = () => {
  const navigate = useNavigate();
  const [submissions, setSubmissions] = useState<AgentSubmission[]>([]);
  const [loadingSubmissions, setLoadingSubmissions] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [pendingCount, setPendingCount] = useState(0);

  useEffect(() => {
    listMySubmissions()
      .then(setSubmissions)
      .catch((err) => {
        setLoadError(err instanceof Error ? err.message : 'Failed to load submissions');
      })
      .finally(() => setLoadingSubmissions(false));
  }, []);

  useEffect(() => {
    import('@tauri-apps/api/core')
      .then(({ invoke }) => invoke<Array<{ id: string }>>('list_pending_recordings'))
      .then((list) => setPendingCount(list.length))
      .catch(() => setPendingCount(0));
  }, []);

  return (
    <div className="p-5 max-w-2xl mx-auto">
      {/* Hero actions */}
      <div className="grid grid-cols-2 gap-3 mb-4">
        <button
          onClick={() => navigate('/record')}
          className="group relative overflow-hidden rounded-2xl bg-gradient-to-br from-red-500 to-red-600 p-5 text-left text-white shadow-lg shadow-red-500/20 hover:shadow-xl hover:shadow-red-500/25 hover:scale-[1.01] transition-all duration-200"
        >
          <div className="absolute top-3 right-3 w-20 h-20 rounded-full bg-white/10 -mr-6 -mt-6" />
          <div className="w-10 h-10 rounded-xl bg-white/20 flex items-center justify-center mb-3">
            <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
              <circle cx="12" cy="12" r="10" />
              <circle cx="12" cy="12" r="4" fill="currentColor" />
            </svg>
          </div>
          <h3 className="font-semibold text-[15px]">New Recording</h3>
          <p className="text-[12px] text-white/70 mt-0.5">Screen + audio capture</p>
        </button>

        <button
          onClick={() => navigate('/submit')}
          className="group relative overflow-hidden rounded-2xl bg-gradient-to-br from-brand-500 to-brand-700 p-5 text-left text-white shadow-lg shadow-brand-500/20 hover:shadow-xl hover:shadow-brand-500/25 hover:scale-[1.01] transition-all duration-200"
        >
          <div className="absolute top-3 right-3 w-20 h-20 rounded-full bg-white/10 -mr-6 -mt-6" />
          <div className="w-10 h-10 rounded-xl bg-white/20 flex items-center justify-center mb-3">
            <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" d="M12 4.5v15m7.5-7.5h-15" />
            </svg>
          </div>
          <h3 className="font-semibold text-[15px]">New Submission</h3>
          <p className="text-[12px] text-white/70 mt-0.5">Feature request or bug</p>
        </button>
      </div>

      {/* Pending recordings */}
      <button
        onClick={() => navigate('/recordings')}
        className="flex items-center justify-between w-full px-4 py-3 rounded-xl border border-slate-200 bg-white hover:border-slate-300 hover:shadow-sm transition text-left mb-6"
      >
        <div>
          <div className="text-[14px] font-medium text-slate-900">Pending Recordings</div>
          <div className="text-[12px] text-slate-500">
            {pendingCount > 0
              ? `${pendingCount} recording${pendingCount === 1 ? '' : 's'} awaiting review`
              : 'No pending recordings'}
          </div>
        </div>
        {pendingCount > 0 && (
          <span className="inline-flex items-center justify-center min-w-[24px] h-6 px-2 rounded-full bg-red-500 text-white text-[11px] font-semibold">
            {pendingCount}
          </span>
        )}
      </button>

      {/* Recent submissions */}
      <div className="bg-white rounded-2xl shadow-sm ring-1 ring-slate-900/5">
        <div className="px-5 py-4 border-b border-slate-100">
          <h3 className="text-[14px] font-semibold text-slate-900">Recent Submissions</h3>
        </div>

        <div className="divide-y divide-slate-50">
          {loadingSubmissions ? (
            <div className="px-5 py-8 text-center">
              <div className="w-6 h-6 border-2 border-slate-200 border-t-brand-500 rounded-full animate-spin mx-auto" />
            </div>
          ) : loadError ? (
            <div className="px-5 py-6 text-center">
              <p className="text-[13px] text-red-500">{loadError}</p>
            </div>
          ) : submissions.length === 0 ? (
            <div className="px-5 py-8 text-center">
              <div className="w-10 h-10 rounded-full bg-slate-100 flex items-center justify-center mx-auto mb-2.5">
                <svg className="w-5 h-5 text-slate-400" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor">
                  <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 14.25v-2.625a3.375 3.375 0 0 0-3.375-3.375h-1.5A1.125 1.125 0 0 1 13.5 7.125v-1.5a3.375 3.375 0 0 0-3.375-3.375H8.25m0 12.75h7.5m-7.5 3H12M10.5 2.25H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 0 0-9-9Z" />
                </svg>
              </div>
              <p className="text-[13px] text-slate-500">No submissions yet</p>
              <p className="text-[12px] text-slate-400 mt-0.5">Create a recording or submission to get started</p>
            </div>
          ) : (
            submissions.slice(0, 10).map((sub) => (
              <div
                key={sub.id}
                className="flex items-center gap-3 px-5 py-3 hover:bg-slate-50/50 transition"
              >
                <div className={`w-2 h-2 rounded-full flex-shrink-0 ${COLUMN_DOTS[sub.columnKey] ?? 'bg-slate-400'}`} />
                <div className="min-w-0 flex-1">
                  <p className="text-[13px] font-medium text-slate-800 truncate">{sub.title}</p>
                  <p className="text-[11px] text-slate-400">
                    {new Date(sub.createdAt).toLocaleDateString(undefined, {
                      month: 'short',
                      day: 'numeric',
                    })}
                  </p>
                </div>
                <span className="text-[11px] font-medium text-slate-500 bg-slate-100 rounded-full px-2.5 py-0.5 whitespace-nowrap">
                  {COLUMN_LABELS[sub.columnKey] ?? sub.columnKey}
                </span>
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  );
};
