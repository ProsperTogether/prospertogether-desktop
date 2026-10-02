import { describe, expect, it } from 'vitest';

import badSchema from '../../tests/fixtures/evidence/v1/bad-schema-version.json';
import badTimezone from '../../tests/fixtures/evidence/v1/bad-timezone.json';
import fullFixture from '../../tests/fixtures/evidence/v1/full.json';
import minimalFixture from '../../tests/fixtures/evidence/v1/minimal.json';
import missingMediaFixture from '../../tests/fixtures/evidence/v1/missing-media.json';
import redactedLeakFixture from '../../tests/fixtures/evidence/v1/redacted-leak.json';
import type {
  EvidenceBundle,
  EvidenceDeviceIdentity,
  EvidenceManifest,
  EvidenceValidationContext,
} from '../types/evidenceBundle';
import {
  assertReviewedBeforeTaskCreate,
  claimImportedBundleId,
  createTaskDraftFromBundle,
  mediaRefForBytes,
  registerImportedBundleId,
  signManifest,
  signingPayload,
  validateEvidenceBundle,
} from './evidenceBundle';
import { hmacSha256Hex, sha256Hex, utf8Bytes } from './evidenceCrypto';

const TEST_IDENTITY: EvidenceDeviceIdentity = {
  deviceId: 'dev-fixture-1',
  deviceKeyId: 'key-fixture-1',
  hmacKeyHex: '11'.repeat(32),
};

const ALT_IDENTITY: EvidenceDeviceIdentity = {
  deviceId: 'dev-other',
  deviceKeyId: 'key-other',
  hmacKeyHex: '22'.repeat(32),
};

function asManifest(value: unknown): EvidenceManifest {
  return JSON.parse(JSON.stringify(value)) as EvidenceManifest;
}

function ctxFor(identity: EvidenceDeviceIdentity, imported: string[] = [], checkDuplicateImport = true): EvidenceValidationContext {
  return {
    trustedKeyIds: [identity.deviceKeyId],
    importedBundleIds: imported,
    deviceKeys: { [identity.deviceKeyId]: identity.hmacKeyHex },
    checkDuplicateImport,
  };
}

function signedBundle(
  manifest: EvidenceManifest,
  artifacts: Record<string, Uint8Array>,
  identity: EvidenceDeviceIdentity = TEST_IDENTITY,
): EvidenceBundle {
  const aligned: EvidenceManifest = {
    ...manifest,
    source: {
      ...manifest.source,
      deviceId: identity.deviceId,
      deviceKeyId: identity.deviceKeyId,
    },
  };
  return {
    manifest: signManifest(aligned, identity, '2026-08-11T19:01:00.000Z'),
    artifacts,
  };
}

describe('evidence crypto vectors', () => {
  it('matches SHA-256 empty and abc vectors', () => {
    expect(sha256Hex(new Uint8Array())).toBe(
      'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855',
    );
    expect(sha256Hex(utf8Bytes('abc'))).toBe(
      'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad',
    );
  });

  it('matches HMAC-SHA256 empty-key empty-message vector', () => {
    expect(hmacSha256Hex(new Uint8Array(), new Uint8Array())).toBe(
      'b613679a0814d9ec772f95d778c35fc5ff1697c493715653c6c712144292c5ad',
    );
  });
});

describe('EvidenceBundle v1 fixtures', () => {
  it('accepts a signed minimal bundle without treating export as import', () => {
    const bundle = signedBundle(asManifest(minimalFixture), { 'art-1': new Uint8Array() });
    const report = validateEvidenceBundle(bundle, ctxFor(TEST_IDENTITY, [bundle.manifest.bundleId], false));
    expect(report.ok).toBe(true);
  });

  it('accepts a signed full bundle with redacted segment and separate media', () => {
    const identity = {
      deviceId: 'dev-fixture-2',
      deviceKeyId: 'key-fixture-2',
      hmacKeyHex: '33'.repeat(32),
    };
    const bundle = signedBundle(
      asManifest(fullFixture),
      { 'art-rec': utf8Bytes('abc'), 'art-shot': new Uint8Array() },
      identity,
    );
    const report = validateEvidenceBundle(bundle, ctxFor(identity));
    expect(report.ok).toBe(true);
    expect(bundle.manifest.transcriptSegments[1]?.text).toBe('[REDACTED]');
  });

  it('rejects an unsupported schema version', () => {
    const bundle = signedBundle(asManifest(badSchema), {});
    expect(validateEvidenceBundle(bundle, ctxFor(TEST_IDENTITY)).errors.some((e) => e.code === 'unsupported_schema_version')).toBe(true);
  });

  it('rejects a non-IANA timezone', () => {
    const bundle = signedBundle(asManifest(badTimezone), {});
    expect(validateEvidenceBundle(bundle, ctxFor(TEST_IDENTITY)).errors.some((e) => e.code === 'invalid_timezone')).toBe(true);
  });

  it('rejects a redacted segment that still contains text', () => {
    const bundle = signedBundle(asManifest(redactedLeakFixture), {});
    expect(validateEvidenceBundle(bundle, ctxFor(TEST_IDENTITY)).errors.some((e) => e.code === 'redacted_segment_leaked')).toBe(true);
  });

  it('rejects missing media artifacts', () => {
    const bundle = signedBundle(asManifest(missingMediaFixture), {});
    expect(validateEvidenceBundle(bundle, ctxFor(TEST_IDENTITY)).errors.some((e) => e.code === 'missing_media')).toBe(true);
  });

  it('rejects a content hash mismatch', () => {
    const bundle = signedBundle(asManifest(minimalFixture), { 'art-1': utf8Bytes('tampered-bytes') });
    expect(validateEvidenceBundle(bundle, ctxFor(TEST_IDENTITY)).errors.some((e) => e.code === 'content_hash_mismatch')).toBe(true);
  });

  it('rejects invalid timestamps and capture ranges beyond the bounded window', () => {
    const invalidDate = asManifest(minimalFixture);
    invalidDate.timeRange.startedAt = '2026-99-99T18:55:00.000Z';
    const invalidReport = validateEvidenceBundle(
      signedBundle(invalidDate, { 'art-1': new Uint8Array() }),
      ctxFor(TEST_IDENTITY, [], false),
    );
    expect(invalidReport.errors.some((e) => e.code === 'invalid_bundle')).toBe(true);

    const tooLong = asManifest(minimalFixture);
    tooLong.timeRange.endedAt = '2026-08-20T19:00:00.000Z';
    const longReport = validateEvidenceBundle(
      signedBundle(tooLong, { 'art-1': new Uint8Array() }),
      ctxFor(TEST_IDENTITY, [], false),
    );
    expect(longReport.errors.some((e) => e.code === 'bound_exceeded')).toBe(true);
  });

  it('rejects unsafe media paths and oversized in-memory artifacts', () => {
    const unsafe = asManifest(minimalFixture);
    unsafe.media[0].relativePath = 'C:/outside.webm';
    const unsafeReport = validateEvidenceBundle(
      signedBundle(unsafe, { 'art-1': new Uint8Array() }),
      ctxFor(TEST_IDENTITY, [], false),
    );
    expect(unsafeReport.errors.some((e) => e.code === 'invalid_bundle')).toBe(true);

    const oversized = asManifest(minimalFixture);
    oversized.media[0].byteLength = Number.MAX_SAFE_INTEGER;
    const oversizedReport = validateEvidenceBundle(
      signedBundle(oversized, { 'art-1': new Uint8Array() }),
      ctxFor(TEST_IDENTITY, [], false),
    );
    expect(oversizedReport.errors.some((e) => e.code === 'bound_exceeded')).toBe(true);
  });

  it('rejects a tampered signed manifest', () => {
    const bundle = signedBundle(asManifest(minimalFixture), { 'art-1': new Uint8Array() });
    bundle.manifest.proposedTask = { ...bundle.manifest.proposedTask, title: 'tampered title' };
    expect(validateEvidenceBundle(bundle, ctxFor(TEST_IDENTITY)).errors.some((e) => e.code === 'tampered_manifest')).toBe(true);
  });

  it('rejects an untrusted signer', () => {
    const bundle = signedBundle(asManifest(minimalFixture), { 'art-1': new Uint8Array() });
    expect(validateEvidenceBundle(bundle, ctxFor(ALT_IDENTITY)).errors.some((e) => e.code === 'untrusted_signer')).toBe(true);
  });

  it('rejects a duplicate import only when checkDuplicateImport is enabled', () => {
    const bundle = signedBundle(asManifest(minimalFixture), { 'art-1': new Uint8Array() });
    const first = registerImportedBundleId([], bundle.manifest.bundleId);
    expect(first.duplicate).toBe(false);
    expect(registerImportedBundleId(first.next, bundle.manifest.bundleId).duplicate).toBe(true);
    expect(
      validateEvidenceBundle(bundle, ctxFor(TEST_IDENTITY, first.next, true)).errors.some((e) => e.code === 'duplicate_import'),
    ).toBe(true);
  });

  it('claims an import atomically and keeps export identity separate', () => {
    const first = claimImportedBundleId({ bundleIds: [] }, 'bundle-1');
    expect(first.accepted).toBe(true);
    const second = claimImportedBundleId(first.index, 'bundle-1');
    expect(second.accepted).toBe(false);
    expect(second.index.bundleIds).toEqual(['bundle-1']);

    const bundle = signedBundle(asManifest(minimalFixture), { 'art-1': new Uint8Array() });
    expect(
      validateEvidenceBundle(bundle, ctxFor(TEST_IDENTITY, ['11111111-1111-4111-8111-111111111111'], false)).ok,
    ).toBe(true);
  });

  it('refuses to create a Task draft before privacy review', () => {
    const unsigned = asManifest(minimalFixture);
    unsigned.review = { privacyReviewed: false, reviewedAt: null, reviewerUserId: null };
    const bundle = signedBundle(unsigned, { 'art-1': new Uint8Array() });
    expect(assertReviewedBeforeTaskCreate(bundle.manifest).some((e) => e.code === 'review_required')).toBe(true);
    const { draft } = createTaskDraftFromBundle(bundle, ctxFor(TEST_IDENTITY, [], false));
    expect(draft).toBeNull();
  });

  it('creates a personal Task draft only after review, never a running Task', () => {
    const bundle = signedBundle(asManifest(minimalFixture), { 'art-1': new Uint8Array() });
    const { draft, report } = createTaskDraftFromBundle(bundle, ctxFor(TEST_IDENTITY, [], false));
    expect(report.ok).toBe(true);
    expect(draft?.scope).toBe('personal');
    expect(draft && 'running' in draft).toBe(false);
  });

  it('persists only reviewed selected media and transcript ids in the draft', () => {
    const bundle = signedBundle(asManifest(fullFixture), { 'art-rec': utf8Bytes('abc'), 'art-shot': new Uint8Array() }, {
      deviceId: 'dev-fixture-2',
      deviceKeyId: 'key-fixture-2',
      hmacKeyHex: '33'.repeat(32),
    });
    const { draft, report } = createTaskDraftFromBundle(
      bundle,
      ctxFor({ deviceId: 'dev-fixture-2', deviceKeyId: 'key-fixture-2', hmacKeyHex: '33'.repeat(32) }, [], false),
      'managed',
      'project/workspace',
      'fixture-provider',
      { selectedMediaIds: ['media-shot'], selectedTranscriptIds: ['seg-1'] },
    );
    expect(report.ok).toBe(true);
    expect(draft?.selectedMediaIds).toEqual(['media-shot']);
    expect(draft?.selectedTranscriptIds).toEqual(['seg-1']);
  });

  it('rejects selection of missing or redacted evidence', () => {
    const bundle = signedBundle(asManifest(fullFixture), { 'art-rec': utf8Bytes('abc'), 'art-shot': new Uint8Array() }, {
      deviceId: 'dev-fixture-2',
      deviceKeyId: 'key-fixture-2',
      hmacKeyHex: '33'.repeat(32),
    });
    const identity = { deviceId: 'dev-fixture-2', deviceKeyId: 'key-fixture-2', hmacKeyHex: '33'.repeat(32) };
    const { draft, report } = createTaskDraftFromBundle(
      bundle,
      ctxFor(identity, [], false),
      'personal',
      null,
      null,
      { selectedMediaIds: ['missing-media'], selectedTranscriptIds: ['seg-2'] },
    );
    expect(draft).toBeNull();
    expect(report.errors.some((e) => e.code === 'invalid_bundle')).toBe(true);
  });

  it('uses an explicit null signature in the canonical signing payload', () => {
    const unsigned = asManifest(minimalFixture);
    unsigned.signature = null;
    const payload = signingPayload(unsigned);
    expect(payload).toContain('"signature":null');
    expect(payload.indexOf('"signature":null')).toBe(payload.lastIndexOf('"signature":null'));
  });

  it('fails closed when selected media requires encrypted-object handoff', () => {
    const manifest = asManifest(minimalFixture);
    manifest.media[0].protection = 'encryptedObjectHandoffRequired';
    const bundle = signedBundle(manifest, {});
    const { draft, report } = createTaskDraftFromBundle(bundle, ctxFor(TEST_IDENTITY, [], false));
    expect(draft).toBeNull();
    expect(report.errors.some((e) => e.code === 'encrypted_object_handoff_required')).toBe(true);
  });

  it('keeps media hashes separate from the manifest body', () => {
    const bytes = utf8Bytes('webm-bytes');
    const ref = mediaRefForBytes({
      id: 'm1',
      kind: 'recording',
      artifactId: 'a1',
      bytes,
      mimeType: 'video/webm',
      fileName: 'recording.webm',
      protection: 'none',
    });
    const manifest = asManifest(minimalFixture);
    manifest.media = [ref];
    const bundle = signedBundle(manifest, { a1: bytes });
    expect(validateEvidenceBundle(bundle, ctxFor(TEST_IDENTITY, [], false)).ok).toBe(true);
    expect(JSON.stringify(bundle.manifest)).not.toContain('webm-bytes');
  });
});
