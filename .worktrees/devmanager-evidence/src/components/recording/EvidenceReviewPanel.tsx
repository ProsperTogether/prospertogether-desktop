import type { EvidenceProposedTask } from '../../types/evidenceBundle';
import type { ReviewGateState } from '../../hooks/useEvidenceReview';

interface EvidenceReviewPanelProps {
  gate: ReviewGateState;
  proposedTask: EvidenceProposedTask;
  onProposedTaskChange: (task: EvidenceProposedTask) => void;
  acceptanceText: string;
  onAcceptanceTextChange: (value: string) => void;
  stepsText: string;
  onStepsTextChange: (value: string) => void;
  scope: 'personal' | 'managed';
  onScopeChange: (scope: 'personal' | 'managed') => void;
  projectWorkspace: string;
  onProjectWorkspaceChange: (value: string) => void;
  provider: string;
  onProviderChange: (value: string) => void;
  onExport: () => void;
  onConfirm: () => void;
  onContinue: () => void;
  busy?: boolean;
}

export function EvidenceReviewPanel({
  gate,
  proposedTask,
  onProposedTaskChange,
  acceptanceText,
  onAcceptanceTextChange,
  stepsText,
  onStepsTextChange,
  scope,
  onScopeChange,
  projectWorkspace,
  onProjectWorkspaceChange,
  provider,
  onProviderChange,
  onExport,
  onConfirm,
  onContinue,
  busy,
}: EvidenceReviewPanelProps) {
  const reviewing = gate.status === 'needs_review' || gate.status === 'reviewed';
  const errors = gate.status === 'blocked' ? gate.report.errors : [];
  const handoff =
    gate.status === 'needs_review' || gate.status === 'reviewed'
      ? gate.result.encryptedObjectHandoffRequired
      : false;

  return (
    <div className="rounded-xl border border-slate-200 p-4 mb-4">
      <h3 className="text-[13px] font-semibold text-slate-800 mb-1">Evidence bundle</h3>
      <p className="text-[12px] text-slate-500 mb-3">
        Review title, summary, acceptance criteria, steps, selected evidence, Personal/Managed
        scope, project/workspace, and provider before creating a Task draft. Media stays hashed
        and separate from the manifest.
      </p>

      {gate.status === 'error' && (
        <p className="text-[13px] text-red-600 mb-3">{gate.message}</p>
      )}
      {gate.status === 'blocked' && (
        <ul className="text-[12px] text-red-600 mb-3 list-disc pl-4">
          {errors.map((issue) => (
            <li key={`${issue.code}-${issue.message}`}>{issue.code}: {issue.message}</li>
          ))}
        </ul>
      )}
      {handoff && (
        <p className="text-[12px] text-amber-700 mb-3">
          Large media requires an encrypted-object handoff; source recording bytes are retained
          until that facility confirms.
        </p>
      )}

      {reviewing && (
        <div className="space-y-2 mb-3">
          <label className="block text-[11px] uppercase tracking-wide text-slate-400">
            Proposed title
            <input
              value={proposedTask.title}
              onChange={(e) => onProposedTaskChange({ ...proposedTask, title: e.target.value })}
              className="mt-1 w-full rounded-lg border border-slate-200 px-3 py-2 text-[13px] text-slate-900"
            />
          </label>
          <label className="block text-[11px] uppercase tracking-wide text-slate-400">
            Summary
            <textarea
              value={proposedTask.summary}
              onChange={(e) => onProposedTaskChange({ ...proposedTask, summary: e.target.value })}
              className="mt-1 w-full rounded-lg border border-slate-200 px-3 py-2 text-[13px] text-slate-900 min-h-[72px]"
            />
          </label>
          <label className="block text-[11px] uppercase tracking-wide text-slate-400">
            Acceptance criteria (one per line)
            <textarea
              value={acceptanceText}
              onChange={(e) => onAcceptanceTextChange(e.target.value)}
              className="mt-1 w-full rounded-lg border border-slate-200 px-3 py-2 text-[13px] text-slate-900 min-h-[64px]"
            />
          </label>
          <label className="block text-[11px] uppercase tracking-wide text-slate-400">
            Steps (one per line)
            <textarea
              value={stepsText}
              onChange={(e) => onStepsTextChange(e.target.value)}
              className="mt-1 w-full rounded-lg border border-slate-200 px-3 py-2 text-[13px] text-slate-900 min-h-[64px]"
            />
          </label>
          {(gate.status === 'needs_review' || gate.status === 'reviewed') && (
            <div className="text-[12px] text-slate-600 rounded-lg border border-slate-100 bg-slate-50 px-3 py-2">
              <div className="text-[11px] uppercase tracking-wide text-slate-400 mb-1">Selected evidence</div>
              <p>
                Media: {gate.result.manifest.media.map((m) => m.id).join(', ') || '(none)'}
              </p>
              <p>
                Transcript:{' '}
                {gate.result.manifest.transcriptSegments
                  .filter((s) => !s.redacted)
                  .map((s) => s.id)
                  .join(', ') || '(none)'}
              </p>
            </div>
          )}
          <label className="block text-[11px] uppercase tracking-wide text-slate-400">
            Scope
            <select
              value={scope}
              onChange={(e) => onScopeChange(e.target.value as 'personal' | 'managed')}
              className="mt-1 w-full rounded-lg border border-slate-200 px-3 py-2 text-[13px] text-slate-900"
            >
              <option value="personal">Personal</option>
              <option value="managed">Managed</option>
            </select>
          </label>
          <label className="block text-[11px] uppercase tracking-wide text-slate-400">
            Project / workspace
            <input
              value={projectWorkspace}
              onChange={(e) => onProjectWorkspaceChange(e.target.value)}
              className="mt-1 w-full rounded-lg border border-slate-200 px-3 py-2 text-[13px] text-slate-900"
            />
          </label>
          <label className="block text-[11px] uppercase tracking-wide text-slate-400">
            Provider
            <input
              value={provider}
              onChange={(e) => onProviderChange(e.target.value)}
              className="mt-1 w-full rounded-lg border border-slate-200 px-3 py-2 text-[13px] text-slate-900"
            />
          </label>
        </div>
      )}


      <div className="flex flex-col gap-2">
        {gate.status === 'exporting' && (
          <p className="text-[13px] text-slate-500">Working on signed evidence…</p>
        )}
        {(gate.status === 'idle' || gate.status === 'error' || gate.status === 'blocked') && (
          <button
            type="button"
            onClick={onExport}
            disabled={busy}
            className="w-full px-4 py-2.5 bg-white text-slate-800 text-[13px] font-medium rounded-lg border border-slate-200 hover:bg-slate-50 disabled:opacity-50"
          >
            Prepare task from evidence
          </button>
        )}
        {gate.status === 'needs_review' && (
          <button
            type="button"
            onClick={onConfirm}
            disabled={busy}
            className="w-full px-4 py-2.5 bg-slate-900 text-white text-[13px] font-medium rounded-lg disabled:opacity-50"
          >
            Confirm review (command receipt)
          </button>
        )}
        {gate.status === 'reviewed' && (
          <button
            type="button"
            onClick={onContinue}
            className="w-full px-4 py-2.5 bg-slate-900 text-white text-[13px] font-medium rounded-lg"
          >
            Continue with draft receipt
          </button>
        )}
      </div>
    </div>
  );
}
