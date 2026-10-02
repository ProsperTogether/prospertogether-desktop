import { useCallback, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

import { emptyProposedTask } from '../services/evidenceBundle';
import type {
  EvidenceExportResult,
  EvidenceProposedTask,
  EvidenceReviewReceipt,
  EvidenceValidationReport,
} from '../types/evidenceBundle';

export type ReviewGateState =
  | { status: 'idle' }
  | { status: 'exporting' }
  | { status: 'needs_review'; result: EvidenceExportResult; report: EvidenceValidationReport }
  | { status: 'blocked'; report: EvidenceValidationReport }
  | { status: 'reviewed'; result: EvidenceExportResult; receipt: EvidenceReviewReceipt }
  | { status: 'error'; message: string };

export function canCreateTask(state: ReviewGateState): boolean {
  return state.status === 'reviewed';
}

export function useEvidenceReview(recordingId: string | undefined) {
  const [gate, setGate] = useState<ReviewGateState>({ status: 'idle' });
  const [proposedTask, setProposedTask] = useState<EvidenceProposedTask>(emptyProposedTask());
  const [acceptanceText, setAcceptanceText] = useState('');
  const [stepsText, setStepsText] = useState('');
  const [scope, setScope] = useState<'personal' | 'managed'>('personal');
  const [projectWorkspace, setProjectWorkspace] = useState('');
  const [provider, setProvider] = useState('');

  const busy = gate.status === 'exporting';

  const exportBundle = useCallback(async () => {
    if (!recordingId) {
      setGate({ status: 'error', message: 'Recording id is required' });
      return;
    }
    setGate({ status: 'exporting' });
    try {
      const result = await invoke<EvidenceExportResult>('export_evidence_bundle', {
        recordingId,
        proposedTask: {
          ...proposedTask,
          acceptanceCriteria: acceptanceText
            .split('\n')
            .map((s) => s.trim())
            .filter(Boolean),
          steps: stepsText
            .split('\n')
            .map((s) => s.trim())
            .filter(Boolean),
        },
        timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC',
        privacyLabels: ['internal_only'],
        privacyReviewed: false,
      });
      const report = await invoke<EvidenceValidationReport>('validate_evidence_bundle', {
        bundle: result.manifest,
        recordingId,
        checkDuplicateImport: false,
      });
      if (!report.ok) {
        setGate({ status: 'blocked', report });
        return;
      }
      setProposedTask(result.manifest.proposedTask);
      setAcceptanceText(result.manifest.proposedTask.acceptanceCriteria.join('\n'));
      setStepsText(result.manifest.proposedTask.steps.join('\n'));
      setGate({ status: 'needs_review', result, report });
    } catch (err) {
      setGate({ status: 'error', message: err instanceof Error ? err.message : String(err) });
    }
  }, [acceptanceText, proposedTask, recordingId, stepsText]);

  const confirmReview = useCallback(async () => {
    if (!recordingId || gate.status !== 'needs_review') return;
    setGate({ status: 'exporting' });
    try {
      const task: EvidenceProposedTask = {
        ...proposedTask,
        acceptanceCriteria: acceptanceText
          .split('\n')
          .map((s) => s.trim())
          .filter(Boolean),
        steps: stepsText
          .split('\n')
          .map((s) => s.trim())
          .filter(Boolean),
      };
      const receipt = await invoke<EvidenceReviewReceipt>('confirm_evidence_review', {
        request: {
          bundle: gate.result.manifest,
          recordingId,
          scope,
          projectWorkspace: projectWorkspace || null,
          provider: provider || null,
          selectedMediaIds: gate.result.manifest.media.map((m) => m.id),
          selectedTranscriptIds: gate.result.manifest.transcriptSegments
            .filter((s) => !s.redacted)
            .map((s) => s.id),
          proposedTask: task,
          reviewerUserId: 'local-reviewer',
        },
      });
      setGate({ status: 'reviewed', result: gate.result, receipt });
    } catch (err) {
      setGate({ status: 'error', message: err instanceof Error ? err.message : String(err) });
    }
  }, [
    acceptanceText,
    gate,
    projectWorkspace,
    proposedTask,
    provider,
    recordingId,
    scope,
    stepsText,
  ]);

  return {
    gate,
    proposedTask,
    setProposedTask,
    acceptanceText,
    setAcceptanceText,
    stepsText,
    setStepsText,
    scope,
    setScope,
    projectWorkspace,
    setProjectWorkspace,
    provider,
    setProvider,
    exportBundle,
    confirmReview,
    canCreateTask: canCreateTask(gate),
    busy,
    receipt: gate.status === 'reviewed' ? gate.receipt : null,
  };
}
