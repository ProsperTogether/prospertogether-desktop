import { describe, expect, it } from 'vitest';

import type { EvidenceReviewReceipt } from '../types/evidenceBundle';
import {
  buildDevManagerConnectEnvelope,
  recordingEvidenceHandoffQuery,
  serializeDevManagerConnectEnvelope,
  toAgentSubmissionFromEnvelope,
} from './devManagerConnect';

function reviewedReceipt(overrides: Partial<EvidenceReviewReceipt> = {}): EvidenceReviewReceipt {
  return {
    receiptId: 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa',
    bundleId: 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb',
    reviewedAt: '2026-08-11T19:05:00.000Z',
    draft: {
      sourceBundleId: 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb',
      title: 'Fix login timeout',
      summary: 'Session evidence for the timeout.',
      acceptanceCriteria: ['User can retry once'],
      steps: ['Open login', 'Wait for timeout'],
      selectedMediaIds: ['media-1'],
      selectedTranscriptIds: ['seg-1'],
      scope: 'managed',
      projectWorkspace: 'portal/agent',
      provider: 'devmanager',
    },
    signature: {
      algorithm: 'hmac-sha256',
      keyId: 'key-fixture-1',
      hex: 'ab'.repeat(32),
      signedAt: '2026-08-11T19:05:00.000Z',
    },
    ...overrides,
  };
}

describe('DevManager Connect handoff', () => {
  it('builds an opaque envelope from a signed receipt without raw transcript or media bytes', () => {
    const envelope = buildDevManagerConnectEnvelope(reviewedReceipt());
    expect(envelope.schemaVersion).toBe('devmanager-connect.task-evidence.v1');
    expect(envelope.evidenceReceiptId).toBe('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa');
    expect(envelope.evidenceBundleId).toBe('bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb');
    expect(envelope.evidenceDraft.scope).toBe('managed');
    expect(envelope.evidenceDraft.projectWorkspace).toBe('portal/agent');
    expect(envelope.evidenceDraft.provider).toBe('devmanager');
    expect(envelope.evidenceDraft.selectedMediaIds).toEqual(['media-1']);
    expect(envelope.evidenceDraft.selectedTranscriptIds).toEqual(['seg-1']);
    expect(envelope.receiptSignature.algorithm).toBe('hmac-sha256');

    const serialized = serializeDevManagerConnectEnvelope(envelope);
    expect(serialized).toContain('"schemaVersion":"devmanager-connect.task-evidence.v1"');
    expect(serialized).not.toContain('transcriptSegments');
    expect(serialized).not.toContain('hmacKeyHex');
    expect(serialized).not.toMatch(/data:video|webm-bytes|\[REDACTED\]/);
  });

  it('maps the envelope onto the existing agent submission contract', () => {
    const envelope = buildDevManagerConnectEnvelope(reviewedReceipt());
    const payload = toAgentSubmissionFromEnvelope(envelope, {
      title: 'Fix login timeout',
      description: 'Session evidence for the timeout.',
      priority: 'high',
      recordingId: 'rec-local-1',
    });
    expect(payload).toEqual({
      title: 'Fix login timeout',
      description: 'Session evidence for the timeout.',
      priority: 'high',
      recordingId: 'rec-local-1',
      evidenceReceiptId: envelope.evidenceReceiptId,
      evidenceBundleId: envelope.evidenceBundleId,
      evidenceDraft: envelope.evidenceDraft,
    });
  });

  it('refuses a receipt that is missing a command signature or scoped draft', () => {
    expect(() =>
      buildDevManagerConnectEnvelope(
        reviewedReceipt({
          signature: {
            algorithm: 'hmac-sha256',
            keyId: 'key-fixture-1',
            hex: '',
            signedAt: '2026-08-11T19:05:00.000Z',
          },
        }),
      ),
    ).toThrow(/signed receipt/i);
    expect(() =>
      buildDevManagerConnectEnvelope(
        reviewedReceipt({
          draft: {
            ...reviewedReceipt().draft,
            scope: 'shared',
          },
        }),
      ),
    ).toThrow(/scope/i);
  });

  it('preserves receipt and bundle ids in the local review-to-submit query', () => {
    const query = recordingEvidenceHandoffQuery(reviewedReceipt(), 'rec-local-1');
    expect(query.get('evidenceReceiptId')).toBe('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa');
    expect(query.get('evidenceBundleId')).toBe('bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb');
    expect(query.get('recordingId')).toBe('rec-local-1');
  });
});
