/**
 * Canonical EvidenceBundle v1 service (validation, signing, draft helpers).
 * Tauri export/import/review commands live on the Rust side.
 */

import {
  EVIDENCE_BOUNDS,
  EVIDENCE_BUNDLE_SCHEMA_VERSION,
  EVIDENCE_PRIVACY_LABELS,
  type EvidenceBundle,
  type EvidenceContentHash,
  type EvidenceDraftSelection,
  type EvidenceDeviceIdentity,
  type EvidenceIssue,
  type EvidenceIssueCode,
  type EvidenceManifest,
  type EvidenceMediaRef,
  type EvidenceTaskDraft,
  type EvidenceValidationContext,
  type EvidenceValidationReport,
} from '../types/evidenceBundle';
import {
  canonicalJson,
  hexToBytes,
  hmacSha256Hex,
  sha256Hex,
  utf8Bytes,
} from './evidenceCrypto';

const SHA256_HEX_RE = /^[0-9a-f]{64}$/;
const IANA_TZ_RE = /^[A-Za-z_]+(?:\/[A-Za-z0-9_+\-]+)+$/;
const UUID_RE = /^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/;
const IDENTIFIER_RE = /^[A-Za-z0-9_-]+$/;
const REDACTED_TEXT = '[REDACTED]';

function issue(code: EvidenceIssueCode, message: string): EvidenceIssue {
  return { code, message };
}

export function isValidTimeZone(timeZone: string): boolean {
  if (timeZone === 'UTC' || timeZone === 'Etc/UTC') return true;
  if (!timeZone || timeZone.length > EVIDENCE_BOUNDS.maxTimeZoneChars) return false;
  return IANA_TZ_RE.test(timeZone);
}

export function isSafeRelativePath(path: string): boolean {
  if (!path || path.length > EVIDENCE_BOUNDS.maxPathChars) return false;
  if (path.startsWith('/') || path.startsWith('\\') || path.includes('\\') || path.includes('\0')) {
    return false;
  }
  return !path.split('/').some((p) => p === '' || p === '.' || p === '..' || p.includes(':'));
}

function isSafeIdentifier(value: string): boolean {
  return value.length > 0 && value.length <= EVIDENCE_BOUNDS.maxBundleIdChars && IDENTIFIER_RE.test(value);
}

function codePointLength(value: string): number {
  return Array.from(value).length;
}

function timestampMs(value: string): number | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.(\d{1,3}))?Z$/.exec(value);
  if (!match) return null;
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const hour = Number(match[4]);
  const minute = Number(match[5]);
  const second = Number(match[6]);
  if (year < 1 || year > 9999 || month < 1 || month > 12 || hour > 23 || minute > 59 || second > 59) return null;
  const date = new Date(Date.UTC(year, month - 1, day, hour, minute, second));
  if (
    date.getUTCFullYear() !== year ||
    date.getUTCMonth() !== month - 1 ||
    date.getUTCDate() !== day ||
    date.getUTCHours() !== hour ||
    date.getUTCMinutes() !== minute ||
    date.getUTCSeconds() !== second
  ) {
    return null;
  }
  const fraction = match[7] ? Number(match[7].padEnd(3, '0')) : 0;
  return date.getTime() + fraction;
}

function validateTimestampRange(startedAt: string, endedAt: string, label: string): EvidenceIssue[] {
  const started = timestampMs(startedAt);
  const ended = timestampMs(endedAt);
  if (started === null || ended === null) {
    return [issue('invalid_bundle', `${label} timestamps must be valid ISO-8601 UTC values`)];
  }
  if (ended < started) {
    return [issue('invalid_bundle', `${label} ended_at must be >= started_at`)];
  }
  return [];
}

export function contentHashOf(bytes: Uint8Array): EvidenceContentHash {
  return { algorithm: 'sha256', hex: sha256Hex(bytes) };
}

export function signingPayload(manifest: EvidenceManifest): string {
  return canonicalJson({ ...manifest, signature: null });
}

export function signManifest(
  manifest: EvidenceManifest,
  identity: EvidenceDeviceIdentity,
  signedAt: string,
): EvidenceManifest {
  const unsigned: EvidenceManifest = {
    ...manifest,
    source: {
      ...manifest.source,
      deviceId: identity.deviceId,
      deviceKeyId: identity.deviceKeyId,
    },
    signature: null,
  };
  const hex = hmacSha256Hex(hexToBytes(identity.hmacKeyHex), utf8Bytes(signingPayload(unsigned)));
  return {
    ...unsigned,
    signature: {
      algorithm: 'hmac-sha256',
      keyId: identity.deviceKeyId,
      hex,
      signedAt,
    },
  };
}

function validateStructure(manifest: EvidenceManifest): EvidenceIssue[] {
  const errors: EvidenceIssue[] = [];
  if (manifest.schemaVersion !== EVIDENCE_BUNDLE_SCHEMA_VERSION) {
    errors.push(issue('unsupported_schema_version', `schema_version ${String(manifest.schemaVersion)} is not supported`));
  }
  if (!manifest.bundleId || manifest.bundleId.length > EVIDENCE_BOUNDS.maxBundleIdChars) {
    errors.push(issue('bound_exceeded', 'bundle_id is missing or too long'));
  } else if (!UUID_RE.test(manifest.bundleId)) {
    errors.push(issue('invalid_bundle', 'bundle_id must be a UUID'));
  }
  if (!isValidTimeZone(manifest.timeRange?.timeZone ?? '')) {
    errors.push(issue('invalid_timezone', `time_zone must be UTC or an IANA name, got ${JSON.stringify(manifest.timeRange?.timeZone)}`));
  }
  errors.push(...validateTimestampRange(manifest.timeRange?.startedAt ?? '', manifest.timeRange?.endedAt ?? '', 'time_range'));
  const started = timestampMs(manifest.timeRange?.startedAt ?? '');
  const ended = timestampMs(manifest.timeRange?.endedAt ?? '');
  if (started !== null && ended !== null && ended - started > EVIDENCE_BOUNDS.maxCaptureDurationMs) {
    errors.push(issue('bound_exceeded', `time_range exceeds ${EVIDENCE_BOUNDS.maxCaptureDurationMs}ms`));
  }
  if (timestampMs(manifest.exportedAt ?? '') === null) {
    errors.push(issue('invalid_bundle', 'exported_at must be a valid ISO-8601 UTC timestamp'));
  }
  if (!manifest.source?.deviceId || !manifest.source?.deviceKeyId) {
    errors.push(issue('invalid_bundle', 'source.device_id and source.device_key_id are required'));
  }
  const segments = manifest.transcriptSegments ?? [];
  if (segments.length > EVIDENCE_BOUNDS.maxTranscriptSegments) {
    errors.push(issue('bound_exceeded', `transcript_segments exceeds ${EVIDENCE_BOUNDS.maxTranscriptSegments}`));
  }
  for (const segment of segments) {
    if (!isSafeIdentifier(segment.id)) {
      errors.push(issue('invalid_bundle', 'transcript segment id is missing or too long'));
    }
    if (utf8Bytes(segment.text).length > EVIDENCE_BOUNDS.maxSegmentTextBytes) {
      errors.push(issue('bound_exceeded', `transcript segment ${segment.id} exceeds text bound`));
    }
    errors.push(...validateTimestampRange(segment.startedAt, segment.endedAt, `transcript segment ${segment.id}`));
  }
  const media = manifest.media ?? [];
  if (media.length > EVIDENCE_BOUNDS.maxMediaRefs) errors.push(issue('bound_exceeded', 'media exceeds bound'));
  const ids = new Set<string>();
  const hashes = new Set<string>();
  for (const ref of media) {
    if (ids.has(ref.id)) errors.push(issue('duplicate_media_id', `duplicate media id ${ref.id}`));
    ids.add(ref.id);
    if (!isSafeIdentifier(ref.id) || !isSafeIdentifier(ref.artifactId)) {
      errors.push(issue('invalid_bundle', `media ${ref.id} has an invalid id`));
    }
    if (!['recording', 'screenshot', 'audio'].includes(ref.kind)) {
      errors.push(issue('invalid_bundle', `media ${ref.id} has an unsupported kind`));
    }
    if (ref.contentHash?.algorithm !== 'sha256' || !SHA256_HEX_RE.test(ref.contentHash?.hex ?? '')) {
      errors.push(issue('invalid_bundle', `media ${ref.id} has an invalid content hash`));
    } else if (hashes.has(ref.contentHash.hex)) {
      errors.push(issue('duplicate_media_hash', `duplicate media hash for ${ref.id}`));
    }
    hashes.add(ref.contentHash.hex);
    if (!Number.isSafeInteger(ref.byteLength) || ref.byteLength < 0 || ref.byteLength > EVIDENCE_BOUNDS.maxArtifactBytes) {
      errors.push(issue('bound_exceeded', `media ${ref.id} byte_length is out of bounds`));
    }
    if (!isSafeRelativePath(ref.relativePath)) {
      errors.push(issue('invalid_bundle', `media ${ref.id} relative_path is unsafe`));
    }
    if (ref.fileName !== null && !isSafeRelativePath(ref.fileName)) {
      errors.push(issue('invalid_bundle', `media ${ref.id} file_name is unsafe`));
    }
    if (!['none', 'encryptedObjectHandoffRequired'].includes(ref.protection)) {
      errors.push(issue('invalid_bundle', `media ${ref.id} has an unsupported protection scheme`));
    }
  }
  if ((manifest.windows ?? []).length > EVIDENCE_BOUNDS.maxWindows) {
    errors.push(issue('bound_exceeded', 'windows exceeds bound'));
  }
  const task = manifest.proposedTask;
  if (!task) errors.push(issue('invalid_bundle', 'proposed_task is required'));
  else {
    if (codePointLength(task.title) > EVIDENCE_BOUNDS.maxTitleChars) errors.push(issue('bound_exceeded', 'proposed_task.title exceeds bound'));
    if (utf8Bytes(task.summary).length > EVIDENCE_BOUNDS.maxSummaryBytes) errors.push(issue('bound_exceeded', 'proposed_task.summary exceeds bound'));
    if (task.acceptanceCriteria.length > EVIDENCE_BOUNDS.maxAcceptanceCriteria) errors.push(issue('bound_exceeded', 'acceptance_criteria exceeds bound'));
    if (task.steps.length > EVIDENCE_BOUNDS.maxSteps) errors.push(issue('bound_exceeded', 'steps exceeds bound'));
    for (const criterion of task.acceptanceCriteria) {
      if (codePointLength(criterion) > EVIDENCE_BOUNDS.maxCriterionChars) errors.push(issue('bound_exceeded', 'acceptance criterion exceeds bound'));
    }
    for (const step of task.steps) {
      if (codePointLength(step) > EVIDENCE_BOUNDS.maxStepChars) errors.push(issue('bound_exceeded', 'step exceeds bound'));
    }
  }
  if ((manifest.privacyLabels ?? []).length > EVIDENCE_BOUNDS.maxPrivacyLabels) {
    errors.push(issue('bound_exceeded', 'privacy_labels exceeds bound'));
  }
  for (const label of manifest.privacyLabels ?? []) {
    if (!EVIDENCE_PRIVACY_LABELS.includes(label as never)) {
      errors.push(issue('invalid_bundle', `unknown privacy label ${label}`));
    }
  }
  if (manifest.redactions.length > EVIDENCE_BOUNDS.maxRedactions) {
    errors.push(issue('bound_exceeded', 'redactions exceeds bound'));
  }
  for (const redaction of manifest.redactions) {
    if (!['transcript', 'media', 'window', 'proposed_task'].includes(redaction.target)) {
      errors.push(issue('invalid_bundle', `redaction ${redaction.targetId} has an unsupported target`));
    }
    if (!isSafeIdentifier(redaction.targetId)) {
      errors.push(issue('invalid_bundle', 'redaction target id is missing or too long'));
    }
    if (timestampMs(redaction.redactedAt) === null) {
      errors.push(issue('invalid_bundle', `redaction ${redaction.targetId} has an invalid timestamp`));
    }
  }
  if (manifest.review.reviewedAt !== null && timestampMs(manifest.review.reviewedAt) === null) {
    errors.push(issue('invalid_bundle', 'reviewed_at must be a valid ISO-8601 UTC timestamp'));
  }
  if (manifest.signature !== null) {
    if (!manifest.signature.keyId || !SHA256_HEX_RE.test(manifest.signature.hex)) {
      errors.push(issue('invalid_bundle', 'signature key_id and hex are required'));
    }
    if (timestampMs(manifest.signature.signedAt) === null) {
      errors.push(issue('invalid_bundle', 'signed_at must be a valid ISO-8601 UTC timestamp'));
    }
  }
  return errors;
}

function validateRedactions(manifest: EvidenceManifest): EvidenceIssue[] {
  const errors: EvidenceIssue[] = [];
  for (const segment of manifest.transcriptSegments ?? []) {
    if (segment.redacted && segment.text.length > 0 && segment.text !== REDACTED_TEXT) {
      errors.push(issue('redacted_segment_leaked', `transcript segment ${segment.id} is marked redacted but still contains text`));
    }
  }
  for (const redaction of manifest.redactions ?? []) {
    if (redaction.target === 'media') {
      if (!(manifest.media ?? []).some((media) => media.id === redaction.targetId)) {
        errors.push(issue('invalid_bundle', `redaction target ${redaction.targetId} is missing`));
      }
      continue;
    }
    if (redaction.target !== 'transcript') continue;
    const segment = (manifest.transcriptSegments ?? []).find((s) => s.id === redaction.targetId);
    if (!segment) {
      errors.push(issue('invalid_bundle', `redaction target ${redaction.targetId} is missing`));
      continue;
    }
    if (!segment.redacted || (segment.text.length > 0 && segment.text !== REDACTED_TEXT)) {
      errors.push(issue('redacted_segment_leaked', `redaction ${redaction.targetId} did not clear transcript text`));
    }
  }
  return errors;
}

function validateMedia(manifest: EvidenceManifest, artifacts: Record<string, Uint8Array>): EvidenceIssue[] {
  const errors: EvidenceIssue[] = [];
  for (const ref of manifest.media ?? []) {
    const stored = artifacts[ref.artifactId];
    if (!stored) {
      if (ref.protection === 'encryptedObjectHandoffRequired') continue;
      errors.push(issue('missing_media', `artifact ${ref.artifactId} is not present`));
      continue;
    }
    if (stored.length > EVIDENCE_BOUNDS.maxArtifactBytes) {
      errors.push(issue('bound_exceeded', `artifact ${ref.artifactId} exceeds size bound`));
      continue;
    }
    if (stored.length !== ref.byteLength) {
      errors.push(issue('content_hash_mismatch', `artifact ${ref.artifactId} length ${stored.length} != ${ref.byteLength}`));
    }
    if (sha256Hex(stored) !== ref.contentHash.hex) {
      errors.push(issue('content_hash_mismatch', `artifact ${ref.artifactId} hash does not match the manifest`));
    }
  }
  return errors;
}

function validateSignature(manifest: EvidenceManifest, ctx: EvidenceValidationContext): EvidenceIssue[] {
  const errors: EvidenceIssue[] = [];
  if (!manifest.signature) {
    errors.push(issue('unsigned_manifest', 'manifest is not signed'));
    return errors;
  }
  if (manifest.signature.algorithm !== 'hmac-sha256') {
    errors.push(issue('invalid_bundle', 'signature algorithm must be hmac-sha256'));
  }
  if (!ctx.trustedKeyIds.includes(manifest.signature.keyId)) {
    errors.push(issue('untrusted_signer', `signer ${manifest.signature.keyId} is not in the trust store`));
  }
  const keyHex = ctx.deviceKeys?.[manifest.signature.keyId];
  if (!keyHex) {
    if (ctx.trustedKeyIds.includes(manifest.signature.keyId)) {
      errors.push(issue('untrusted_signer', `trusted key ${manifest.signature.keyId} material is unavailable`));
    }
    return errors;
  }
  const expected = hmacSha256Hex(hexToBytes(keyHex), utf8Bytes(signingPayload(manifest)));
  if (expected !== manifest.signature.hex || manifest.signature.keyId !== manifest.source.deviceKeyId) {
    errors.push(issue('tampered_manifest', 'signature does not match the canonical manifest'));
  }
  return errors;
}

export function validateEvidenceBundle(
  bundle: EvidenceBundle,
  ctx: EvidenceValidationContext,
): EvidenceValidationReport {
  const errors: EvidenceIssue[] = [];
  errors.push(...validateStructure(bundle.manifest));
  errors.push(...validateRedactions(bundle.manifest));
  errors.push(...validateMedia(bundle.manifest, bundle.artifacts));
  errors.push(...validateSignature(bundle.manifest, ctx));
  if (ctx.checkDuplicateImport !== false && ctx.importedBundleIds.includes(bundle.manifest.bundleId)) {
    errors.push(issue('duplicate_import', `bundle ${bundle.manifest.bundleId} was already imported`));
  }
  return { ok: errors.length === 0, errors, bundleId: bundle.manifest.bundleId ?? null };
}

export function assertReviewedBeforeTaskCreate(manifest: EvidenceManifest): EvidenceIssue[] {
  const errors: EvidenceIssue[] = [];
  if (!manifest.review?.privacyReviewed || !manifest.review.reviewedAt) {
    errors.push(issue('review_required', 'privacy review must be completed before creating a Task draft'));
  }
  if (!manifest.signature) {
    errors.push(issue('unsigned_manifest', 'a Task draft requires a signed EvidenceBundle'));
  }
  if (!manifest.proposedTask?.title?.trim()) {
    errors.push(issue('review_required', 'proposed title must be reviewed'));
  }
  return errors;
}

export function createTaskDraftFromBundle(
  bundle: EvidenceBundle,
  ctx: EvidenceValidationContext,
  scope: 'personal' | 'managed' = 'personal',
  projectWorkspace: string | null = null,
  provider: string | null = null,
  selection: EvidenceDraftSelection = {},
): { draft: EvidenceTaskDraft | null; report: EvidenceValidationReport } {
  const report = validateEvidenceBundle(bundle, { ...ctx, checkDuplicateImport: ctx.checkDuplicateImport ?? false });
  const reviewErrors = assertReviewedBeforeTaskCreate(bundle.manifest);
  const errors = [...report.errors, ...reviewErrors];
  const selectedMediaIds = [...(selection.selectedMediaIds ?? bundle.manifest.media.map((m) => m.id))];
  const selectedTranscriptIds = [...(selection.selectedTranscriptIds ?? bundle.manifest.transcriptSegments.filter((s) => !s.redacted).map((s) => s.id))];
  const mediaIds = new Set(bundle.manifest.media.map((m) => m.id));
  const transcriptById = new Map(bundle.manifest.transcriptSegments.map((s) => [s.id, s]));
  if (new Set(selectedMediaIds).size !== selectedMediaIds.length || selectedMediaIds.some((id) => !mediaIds.has(id))) {
    errors.push(issue('invalid_bundle', 'selected media ids must be unique and present in the bundle'));
  }
  if (
    new Set(selectedTranscriptIds).size !== selectedTranscriptIds.length ||
    selectedTranscriptIds.some((id) => !transcriptById.has(id) || transcriptById.get(id)?.redacted)
  ) {
    errors.push(issue('invalid_bundle', 'selected transcript ids must be unique, present, and unredacted'));
  }
  const redactedMediaIds = new Set(
    bundle.manifest.redactions.filter((redaction) => redaction.target === 'media').map((redaction) => redaction.targetId),
  );
  if (selectedMediaIds.some((id) => redactedMediaIds.has(id))) {
    errors.push(issue('invalid_bundle', 'selected media ids must not include redacted media'));
  }
  if (selectedMediaIds.some((id) => bundle.manifest.media.find((media) => media.id === id)?.protection === 'encryptedObjectHandoffRequired')) {
    errors.push(issue('encrypted_object_handoff_required', 'selected media requires an unavailable encrypted-object handoff'));
  }
  if (errors.length > 0) {
    return { draft: null, report: { ok: false, errors, bundleId: bundle.manifest.bundleId ?? null } };
  }
  return {
    draft: {
      sourceBundleId: bundle.manifest.bundleId,
      title: bundle.manifest.proposedTask.title,
      summary: bundle.manifest.proposedTask.summary,
      acceptanceCriteria: [...bundle.manifest.proposedTask.acceptanceCriteria],
      steps: [...bundle.manifest.proposedTask.steps],
      selectedMediaIds,
      selectedTranscriptIds,
      scope,
      projectWorkspace,
      provider,
    },
    report: { ok: true, errors: [], bundleId: bundle.manifest.bundleId },
  };
}

export function registerImportedBundleId(
  imported: string[],
  bundleId: string,
): { next: string[]; duplicate: boolean } {
  const result = claimImportedBundleId({ bundleIds: imported }, bundleId);
  return { next: result.index.bundleIds, duplicate: !result.accepted };
}

/**
 * Synchronous claim used by the local import index. JavaScript runs this
 * critical section without yielding, so a caller cannot observe a half-claim.
 * Persistence layers must still commit the returned index atomically.
 */
export function claimImportedBundleId(
  index: { bundleIds: string[] },
  bundleId: string,
): { index: { bundleIds: string[] }; accepted: boolean } {
  if (index.bundleIds.includes(bundleId)) return { index: { bundleIds: [...index.bundleIds] }, accepted: false };
  return { index: { bundleIds: [...index.bundleIds, bundleId] }, accepted: true };
}

export function mediaRefForBytes(input: {
  id: string;
  kind: EvidenceMediaRef['kind'];
  artifactId: string;
  bytes: Uint8Array;
  mimeType: string;
  fileName: string | null;
  protection?: EvidenceMediaRef['protection'];
}): EvidenceMediaRef {
  return {
    id: input.id,
    kind: input.kind,
    artifactId: input.artifactId,
    relativePath: input.fileName ?? `${input.artifactId}.bin`,
    contentHash: contentHashOf(input.bytes),
    mimeType: input.mimeType,
    byteLength: input.bytes.length,
    fileName: input.fileName,
    protection: input.protection ?? 'none',
  };
}

export function emptyProposedTask() {
  return { title: '', summary: '', acceptanceCriteria: [] as string[], steps: [] as string[] };
}
