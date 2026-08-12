/**
 * Narrow DevManager Connect adapter at the existing app/API boundary.
 * Serializes a verified local review receipt into the canonical agent
 * submission contract. No second protocol and no raw transcript/media.
 */

import type { EvidenceReviewReceipt, EvidenceSignature, EvidenceTaskDraft } from '../types/evidenceBundle';
import { canonicalJson } from './evidenceCrypto';

export const DEVMANAGER_CONNECT_SCHEMA_VERSION = 'devmanager-connect.task-evidence.v1' as const;

const UUID_RE = /^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/;
const SHA256_HEX_RE = /^[0-9a-f]{64}$/;

export interface DevManagerConnectEnvelope {
  schemaVersion: typeof DEVMANAGER_CONNECT_SCHEMA_VERSION;
  evidenceReceiptId: string;
  evidenceBundleId: string;
  evidenceDraft: EvidenceTaskDraft;
  receiptSignature: EvidenceSignature;
}

export interface AgentSubmissionFromEnvelope {
  title: string;
  description?: string;
  priority?: string;
  recordingId?: string;
  evidenceReceiptId: string;
  evidenceBundleId: string;
  evidenceDraft: EvidenceTaskDraft;
}

function assertCommandReceipt(receipt: EvidenceReviewReceipt): void {
  if (!UUID_RE.test(receipt.receiptId) || !UUID_RE.test(receipt.bundleId)) {
    throw new Error('DevManager Connect requires a command-signed receipt with valid ids');
  }
  if (receipt.draft?.sourceBundleId !== receipt.bundleId) {
    throw new Error('DevManager Connect receipt bundle id does not match the draft');
  }
  if (receipt.draft.scope !== 'personal' && receipt.draft.scope !== 'managed') {
    throw new Error('DevManager Connect receipt scope must be personal or managed');
  }
  const signature = receipt.signature;
  if (
    !signature ||
    signature.algorithm !== 'hmac-sha256' ||
    !signature.keyId ||
    !SHA256_HEX_RE.test(signature.hex)
  ) {
    throw new Error('DevManager Connect requires a command-signed receipt');
  }
}

export function buildDevManagerConnectEnvelope(
  receipt: EvidenceReviewReceipt,
): DevManagerConnectEnvelope {
  assertCommandReceipt(receipt);
  return {
    schemaVersion: DEVMANAGER_CONNECT_SCHEMA_VERSION,
    evidenceReceiptId: receipt.receiptId,
    evidenceBundleId: receipt.bundleId,
    evidenceDraft: {
      sourceBundleId: receipt.draft.sourceBundleId,
      title: receipt.draft.title,
      summary: receipt.draft.summary,
      acceptanceCriteria: [...receipt.draft.acceptanceCriteria],
      steps: [...receipt.draft.steps],
      selectedMediaIds: [...receipt.draft.selectedMediaIds],
      selectedTranscriptIds: [...receipt.draft.selectedTranscriptIds],
      scope: receipt.draft.scope,
      projectWorkspace: receipt.draft.projectWorkspace ?? null,
      provider: receipt.draft.provider ?? null,
    },
    receiptSignature: { ...receipt.signature },
  };
}

export function serializeDevManagerConnectEnvelope(envelope: DevManagerConnectEnvelope): string {
  return canonicalJson(envelope);
}

export function toAgentSubmissionFromEnvelope(
  envelope: DevManagerConnectEnvelope,
  input: {
    title: string;
    description?: string;
    priority?: string;
    recordingId?: string;
  },
): AgentSubmissionFromEnvelope {
  return {
    title: input.title,
    description: input.description,
    priority: input.priority,
    recordingId: input.recordingId,
    evidenceReceiptId: envelope.evidenceReceiptId,
    evidenceBundleId: envelope.evidenceBundleId,
    evidenceDraft: envelope.evidenceDraft,
  };
}

export function recordingEvidenceHandoffQuery(
  receipt: EvidenceReviewReceipt,
  recordingId?: string,
): URLSearchParams {
  const envelope = buildDevManagerConnectEnvelope(receipt);
  const query = new URLSearchParams({
    evidenceReceiptId: envelope.evidenceReceiptId,
    evidenceBundleId: envelope.evidenceBundleId,
  });
  if (recordingId) query.set('recordingId', recordingId);
  return query;
}
