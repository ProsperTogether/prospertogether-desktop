import { useEffect, useRef, useState } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';

import { createSubmission } from '../../api/submissions';
import { getRecording, type RecordingDetail } from '../../api/recordings';

const PRIORITIES = [
  { value: 'low', label: 'Low', dot: 'bg-slate-400' },
  { value: 'medium', label: 'Medium', dot: 'bg-yellow-500' },
  { value: 'high', label: 'High', dot: 'bg-orange-500' },
  { value: 'critical', label: 'Critical', dot: 'bg-red-500' },
];

interface SubmitPageProps {
  recordingId?: string | null;
}

function formatUsd(n: number | string | null): string {
  if (n == null) return '';
  const num = typeof n === 'string' ? Number(n) : n;
  if (!Number.isFinite(num)) return '';
  if (num < 0.01) return `$${num.toFixed(4)}`;
  return num.toLocaleString('en-US', { style: 'currency', currency: 'USD' });
}

function formatNumber(n: number | null): string {
  if (n == null) return '0';
  return n.toLocaleString('en-US');
}

export const SubmitPage = ({ recordingId: propRecordingId }: SubmitPageProps) => {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const recordingId = propRecordingId ?? searchParams.get('recordingId');
  const [title, setTitle] = useState('');
  const [description, setDescription] = useState('');
  const [priority, setPriority] = useState('medium');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [recording, setRecording] = useState<RecordingDetail | null>(null);
  const [recordingError, setRecordingError] = useState<string | null>(null);
  // Track whether the user has manually edited any field, so polling does not clobber edits.
  const userEditedRef = useRef({ title: false, description: false, priority: false });

  // Fetch + poll the recording until aiAnalysisStatus reaches a terminal state.
  useEffect(() => {
    if (!recordingId) return;
    let cancelled = false;
    let timeoutId: number | undefined;

    const tick = async () => {
      try {
        const data = await getRecording(recordingId);
        if (cancelled) return;
        setRecording(data);
        setRecordingError(null);

        // Auto-fill form fields from the AI analysis ONCE (only if user hasn't edited).
        if (data.aiAnalysisStatus === 'completed' && data.aiAnalysis) {
          if (!userEditedRef.current.title && data.aiAnalysis.suggestedTitle) {
            setTitle(prev => (prev ? prev : data.aiAnalysis!.suggestedTitle));
          }
          if (!userEditedRef.current.description && data.aiAnalysis.suggestedDescription) {
            setDescription(prev => (prev ? prev : data.aiAnalysis!.suggestedDescription));
          }
          if (!userEditedRef.current.priority && data.aiAnalysis.suggestedPriority) {
            setPriority(prev => (prev !== 'medium' ? prev : data.aiAnalysis!.suggestedPriority));
          }
        }

        // Continue polling while we're waiting for analysis to complete.
        const status = data.aiAnalysisStatus;
        if (status === 'pending' || status === 'processing') {
          timeoutId = window.setTimeout(tick, 2500);
        }
      } catch (err) {
        if (cancelled) return;
        setRecordingError(err instanceof Error ? err.message : String(err));
        // Retry after a short delay so transient network issues don't permanently break.
        timeoutId = window.setTimeout(tick, 5000);
      }
    };

    void tick();
    return () => {
      cancelled = true;
      if (timeoutId) window.clearTimeout(timeoutId);
    };
  }, [recordingId]);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!title.trim()) return;

    setSubmitting(true);
    setError(null);

    try {
      await createSubmission({
        title: title.trim(),
        description: description.trim() || undefined,
        priority,
        recordingId: recordingId ?? undefined,
      });
      navigate('/');
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to submit');
    } finally {
      setSubmitting(false);
    }
  };

  const aiStatus = recording?.aiAnalysisStatus ?? null;
  const aiAnalysis = recording?.aiAnalysis ?? null;

  return (
    <div className="p-5 max-w-3xl mx-auto">
      <div className="mb-5">
        <h1 className="text-lg font-semibold text-slate-900">New Submission</h1>
        <p className="text-[13px] text-slate-500 mt-0.5">Describe a feature request or bug report</p>
      </div>

      {/* AI Scope of Work panel */}
      {recordingId && (
        <div className="mb-4 bg-white rounded-2xl shadow-sm ring-1 ring-slate-900/5 overflow-hidden">
          <div className="px-5 py-3 border-b border-slate-100 flex items-center justify-between">
            <div className="flex items-center gap-2">
              <span className="text-[13px] font-semibold text-slate-700">AI Scope of Work</span>
              {aiStatus === 'pending' && <StatusPill tone="blue" label="Queued" />}
              {aiStatus === 'processing' && <StatusPill tone="blue" label="Analyzing video..." spin />}
              {aiStatus === 'completed' && <StatusPill tone="emerald" label="Ready" />}
              {aiStatus === 'failed' && <StatusPill tone="red" label="Failed" />}
              {aiStatus === 'skipped' && <StatusPill tone="slate" label="Skipped" />}
            </div>
            {aiStatus === 'completed' && recording && (
              <CostFooter recording={recording} />
            )}
          </div>

          {aiStatus === 'failed' && recording?.aiAnalysisError && (
            <div className="px-5 py-3 bg-red-50 border-b border-red-100 text-[13px] text-red-700 whitespace-pre-wrap font-mono">
              {recording.aiAnalysisError}
            </div>
          )}

          {recordingError && (
            <div className="px-5 py-3 bg-red-50 border-b border-red-100 text-[13px] text-red-700">
              Failed to load recording: {recordingError}
            </div>
          )}

          {aiStatus === 'completed' && aiAnalysis && (
            <div className="px-5 py-4 space-y-4">
              {aiAnalysis.summary && (
                <div>
                  <div className="text-[11px] uppercase tracking-wide font-semibold text-slate-500 mb-1">Summary</div>
                  <p className="text-[13px] text-slate-700">{aiAnalysis.summary}</p>
                </div>
              )}

              {aiAnalysis.scopeOfWork && (
                <div>
                  <div className="text-[11px] uppercase tracking-wide font-semibold text-slate-500 mb-1">Scope of Work</div>
                  <div className="text-[13px] text-slate-700 whitespace-pre-wrap">{aiAnalysis.scopeOfWork}</div>
                </div>
              )}

              {aiAnalysis.devChanges && aiAnalysis.devChanges.length > 0 && (
                <div>
                  <div className="text-[11px] uppercase tracking-wide font-semibold text-slate-500 mb-2">
                    Dev Changes ({aiAnalysis.devChanges.length})
                  </div>
                  <ul className="space-y-2">
                    {aiAnalysis.devChanges.map((c, i) => (
                      <li key={i} className="border border-slate-200 rounded-lg px-3 py-2.5 bg-slate-50/50">
                        <div className="flex items-start justify-between gap-2">
                          <div className="text-[13px] font-medium text-slate-800">{c.title}</div>
                          <span className={`text-[10px] uppercase tracking-wide px-1.5 py-0.5 rounded font-semibold ${
                            c.changeType === 'bug' ? 'bg-red-100 text-red-700'
                            : c.changeType === 'feature' ? 'bg-blue-100 text-blue-700'
                            : c.changeType === 'enhancement' ? 'bg-emerald-100 text-emerald-700'
                            : 'bg-slate-100 text-slate-700'
                          }`}>{c.changeType}</span>
                        </div>
                        <p className="text-[12px] text-slate-600 mt-1">{c.description}</p>
                        {c.affectedArea && (
                          <p className="text-[11px] text-slate-500 mt-1">Area: {c.affectedArea}</p>
                        )}
                        {c.visualEvidence && (
                          <p className="text-[11px] text-slate-400 mt-0.5">Evidence: {c.visualEvidence}</p>
                        )}
                      </li>
                    ))}
                  </ul>
                </div>
              )}

              {recording?.transcription && (
                <details className="text-[12px] text-slate-500">
                  <summary className="cursor-pointer hover:text-slate-700">View raw transcript</summary>
                  <p className="mt-2 whitespace-pre-wrap text-slate-600">{recording.transcription}</p>
                </details>
              )}
            </div>
          )}

          {(aiStatus === 'pending' || aiStatus === 'processing') && (
            <div className="px-5 py-6 text-center text-[13px] text-slate-500">
              Analyzing your recording... this typically takes 5–15 seconds.
            </div>
          )}

          {aiStatus === 'skipped' && (
            <div className="px-5 py-4 text-[12px] text-slate-500">
              No transcript or video frames were available, so AI analysis was skipped.
            </div>
          )}
        </div>
      )}

      <form onSubmit={handleSubmit} className="bg-white rounded-2xl shadow-sm ring-1 ring-slate-900/5 p-5 space-y-4">
        {recordingId && (
          <div className="flex items-center gap-2.5 text-[13px] text-emerald-700 bg-emerald-50 border border-emerald-100 rounded-xl px-3.5 py-2.5">
            <svg className="w-4 h-4 flex-shrink-0" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" d="m15.75 10.5 4.72-4.72a.75.75 0 0 1 1.28.53v11.38a.75.75 0 0 1-1.28.53l-4.72-4.72M4.5 18.75h9a2.25 2.25 0 0 0 2.25-2.25v-9a2.25 2.25 0 0 0-2.25-2.25h-9A2.25 2.25 0 0 0 2.25 7.5v9a2.25 2.25 0 0 0 2.25 2.25Z" />
            </svg>
            Recording attached
          </div>
        )}

        <div>
          <label className="block text-[13px] font-medium text-slate-700 mb-1.5">Title</label>
          <input
            type="text"
            className="w-full border border-slate-200 bg-slate-50/50 rounded-lg px-3.5 py-2.5 text-sm text-slate-900 placeholder:text-slate-400 focus:border-brand-500 focus:ring-2 focus:ring-brand-500/20 focus:bg-white transition outline-none"
            placeholder="Brief description of the issue or request"
            value={title}
            onChange={(e) => {
              setTitle(e.target.value);
              userEditedRef.current.title = true;
            }}
            autoFocus
            required
          />
        </div>

        <div>
          <label className="block text-[13px] font-medium text-slate-700 mb-1.5">Description</label>
          <textarea
            className="w-full border border-slate-200 bg-slate-50/50 rounded-lg px-3.5 py-2.5 text-sm text-slate-900 placeholder:text-slate-400 focus:border-brand-500 focus:ring-2 focus:ring-brand-500/20 focus:bg-white transition outline-none resize-none"
            rows={6}
            placeholder="Add details (optional)"
            value={description}
            onChange={(e) => {
              setDescription(e.target.value);
              userEditedRef.current.description = true;
            }}
          />
        </div>

        <div>
          <label className="block text-[13px] font-medium text-slate-700 mb-2">Priority</label>
          <div className="flex gap-2">
            {PRIORITIES.map((p) => (
              <button
                key={p.value}
                type="button"
                onClick={() => {
                  setPriority(p.value);
                  userEditedRef.current.priority = true;
                }}
                className={`flex items-center gap-1.5 px-3 py-1.5 text-[12px] font-medium rounded-lg border transition ${
                  priority === p.value
                    ? 'bg-slate-900 text-white border-slate-900'
                    : 'bg-white text-slate-600 border-slate-200 hover:border-slate-300'
                }`}
              >
                <div className={`w-1.5 h-1.5 rounded-full ${p.dot}`} />
                {p.label}
              </button>
            ))}
          </div>
        </div>

        {error && (
          <div className="rounded-lg bg-red-50 border border-red-100 px-3.5 py-2.5">
            <p className="text-[13px] text-red-600">{error}</p>
          </div>
        )}

        <div className="flex gap-2.5 pt-1">
          <button
            type="submit"
            disabled={submitting || !title.trim()}
            className="flex-1 px-4 py-2.5 bg-gradient-to-b from-brand-500 to-brand-600 text-white text-sm font-medium rounded-lg hover:from-brand-600 hover:to-brand-700 disabled:opacity-50 shadow-sm shadow-brand-600/25 transition"
          >
            {submitting ? 'Submitting...' : 'Submit'}
          </button>
          <button
            type="button"
            onClick={() => navigate('/')}
            className="px-4 py-2.5 text-sm text-slate-600 border border-slate-200 rounded-lg hover:bg-slate-50 transition"
          >
            Cancel
          </button>
        </div>
      </form>
    </div>
  );
};

function StatusPill({ tone, label, spin }: { tone: 'blue' | 'emerald' | 'red' | 'slate'; label: string; spin?: boolean }) {
  const cls = {
    blue: 'bg-blue-50 text-blue-700 border-blue-200',
    emerald: 'bg-emerald-50 text-emerald-700 border-emerald-200',
    red: 'bg-red-50 text-red-700 border-red-200',
    slate: 'bg-slate-50 text-slate-600 border-slate-200',
  }[tone];
  return (
    <span className={`text-[11px] px-2 py-0.5 rounded-full border ${cls} flex items-center gap-1`}>
      {spin && <span className="w-2 h-2 border border-current border-t-transparent rounded-full animate-spin" />}
      {label}
    </span>
  );
}

function CostFooter({ recording }: { recording: RecordingDetail }) {
  const cost = formatUsd(recording.aiAnalysisCostUsd);
  const inputT = formatNumber(recording.aiAnalysisInputTokens);
  const outputT = formatNumber(recording.aiAnalysisOutputTokens);
  const imgT = formatNumber(recording.aiAnalysisImageTokens);
  const latencyS = recording.aiAnalysisLatencyMs ? (recording.aiAnalysisLatencyMs / 1000).toFixed(1) : '?';
  const model = recording.aiAnalysisModel ?? '?';
  return (
    <div className="text-[10px] text-slate-500 tabular-nums" title="Token usage and cost for this analysis">
      {cost} · {inputT} in / {outputT} out / {imgT} img · {model} · {latencyS}s
    </div>
  );
}
