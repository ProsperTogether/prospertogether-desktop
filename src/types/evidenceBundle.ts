/** DevAgent EvidenceBundle v1 — camelCase wire DTO shared with Rust/Portal. */

export const EVIDENCE_BUNDLE_SCHEMA_VERSION = 'evidence-bundle.v1' as const;

export const EVIDENCE_BOUNDS = {
  maxTranscriptSegments: 200,
  maxSegmentTextBytes: 8192,
  maxMediaRefs: 32,
  maxWindows: 16,
  maxTitleChars: 200,
  maxSummaryBytes: 8192,
  maxAcceptanceCriteria: 20,
  maxCriterionChars: 500,
  maxSteps: 50,
  maxStepChars: 500,
  maxRedactions: 100,
  maxPrivacyLabels: 8,
  maxBundleIdChars: 64,
  maxTimeZoneChars: 64,
  maxPathChars: 512,
  maxCaptureDurationMs: 7 * 24 * 60 * 60 * 1000,
  maxArtifactBytes: 512 * 1024 * 1024,
} as const;

export type EvidenceHashAlgorithm = 'sha256';
export type EvidenceSignatureAlgorithm = 'hmac-sha256';
export type EvidenceMediaKind = 'recording' | 'screenshot' | 'audio';
export type EvidenceCaptureMode = 'screen' | 'monitor' | 'window' | 'region';
/** `encryptedObjectHandoffRequired` = no local XOR invent; need audited encrypt facility. */
export type EvidenceProtectionScheme = 'none' | 'encryptedObjectHandoffRequired';
export type EvidenceRedactionTarget = 'transcript' | 'media' | 'window' | 'proposed_task';
export type EvidencePrivacyLabel =
  | 'contains_pii'
  | 'contains_credentials'
  | 'contains_customer_data'
  | 'internal_only'
  | 'public_safe';

export const EVIDENCE_PRIVACY_LABELS: readonly EvidencePrivacyLabel[] = [
  'contains_pii',
  'contains_credentials',
  'contains_customer_data',
  'internal_only',
  'public_safe',
];

export type EvidenceIssueCode =
  | 'unsupported_schema_version'
  | 'invalid_timezone'
  | 'content_hash_mismatch'
  | 'tampered_manifest'
  | 'missing_media'
  | 'redacted_segment_leaked'
  | 'duplicate_import'
  | 'duplicate_media_id'
  | 'duplicate_media_hash'
  | 'untrusted_signer'
  | 'review_required'
  | 'unsigned_manifest'
  | 'bound_exceeded'
  | 'encrypted_object_handoff_required'
  | 'invalid_bundle';

export interface EvidenceContentHash {
  algorithm: EvidenceHashAlgorithm;
  hex: string;
}

export interface EvidenceTimeRange {
  startedAt: string;
  endedAt: string;
  timeZone: string;
}

export interface EvidenceSourceIdentity {
  deviceId: string;
  deviceKeyId: string;
  userId: string | null;
  displayName: string | null;
}

export interface EvidenceTranscriptSegment {
  id: string;
  startedAt: string;
  endedAt: string;
  text: string;
  speaker: string | null;
  redacted: boolean;
}

export interface EvidenceMediaRef {
  id: string;
  kind: EvidenceMediaKind | string;
  artifactId: string;
  relativePath: string;
  contentHash: EvidenceContentHash;
  mimeType: string;
  byteLength: number;
  fileName: string | null;
  protection: EvidenceProtectionScheme | string;
}

export interface EvidenceWindowBounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface EvidenceWindowMetadata {
  title: string | null;
  application: string | null;
  bounds: EvidenceWindowBounds | null;
  captureMode: EvidenceCaptureMode | string;
}

export interface EvidenceProposedTask {
  title: string;
  summary: string;
  acceptanceCriteria: string[];
  steps: string[];
}

export interface EvidenceRedaction {
  target: EvidenceRedactionTarget | string;
  targetId: string;
  reason: string;
  redactedAt: string;
}

export interface EvidenceReview {
  privacyReviewed: boolean;
  reviewedAt: string | null;
  reviewerUserId: string | null;
}

export interface EvidenceSignature {
  algorithm: EvidenceSignatureAlgorithm | string;
  keyId: string;
  hex: string;
  signedAt: string;
}

export interface EvidenceManifest {
  schemaVersion: typeof EVIDENCE_BUNDLE_SCHEMA_VERSION | string;
  bundleId: string;
  exportedAt: string;
  timeRange: EvidenceTimeRange;
  source: EvidenceSourceIdentity;
  transcriptSegments: EvidenceTranscriptSegment[];
  media: EvidenceMediaRef[];
  windows: EvidenceWindowMetadata[];
  proposedTask: EvidenceProposedTask;
  privacyLabels: EvidencePrivacyLabel[] | string[];
  redactions: EvidenceRedaction[];
  review: EvidenceReview;
  signature: EvidenceSignature | null;
}

export interface EvidenceBundle {
  manifest: EvidenceManifest;
  artifacts: Record<string, Uint8Array>;
}

export interface EvidenceIssue {
  code: EvidenceIssueCode | string;
  message: string;
}

export interface EvidenceValidationReport {
  ok: boolean;
  errors: EvidenceIssue[];
  bundleId: string | null;
}

export interface EvidenceDeviceIdentity {
  deviceId: string;
  deviceKeyId: string;
  hmacKeyHex: string;
}

export interface EvidenceValidationContext {
  trustedKeyIds: string[];
  importedBundleIds: string[];
  deviceKeys?: Record<string, string>;
  checkDuplicateImport?: boolean;
}

export interface EvidenceDraftSelection {
  selectedMediaIds?: string[];
  selectedTranscriptIds?: string[];
}

export interface EvidenceTaskDraft {
  sourceBundleId: string;
  title: string;
  summary: string;
  acceptanceCriteria: string[];
  steps: string[];
  selectedMediaIds: string[];
  selectedTranscriptIds: string[];
  scope: 'personal' | 'managed' | string;
  projectWorkspace?: string | null;
  provider?: string | null;
}

export interface EvidenceReviewReceipt {
  receiptId: string;
  bundleId: string;
  reviewedAt: string;
  draft: EvidenceTaskDraft;
  signature: EvidenceSignature;
}

export interface EvidenceExportResult {
  bundleId: string;
  bundleDir: string;
  manifestPath: string;
  artifactsDir: string;
  manifest: EvidenceManifest;
  reviewRequired: boolean;
  encryptedObjectHandoffRequired: boolean;
}
