use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// EvidenceBundle v1 schema id shared with TS/Portal.
pub const EVIDENCE_BUNDLE_SCHEMA_VERSION: &str = "evidence-bundle.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceContentHash {
    pub algorithm: String,
    pub hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceTimeRange {
    pub started_at: String,
    pub ended_at: String,
    pub time_zone: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceSourceIdentity {
    pub device_id: String,
    pub device_key_id: String,
    pub user_id: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceTranscriptSegment {
    pub id: String,
    pub started_at: String,
    pub ended_at: String,
    pub text: String,
    pub speaker: Option<String>,
    pub redacted: bool,
}

/// `protection`:
/// - `none`: hashed source-media ref under the recording root (bytes stay there)
/// - `encryptedObjectHandoffRequired`: separate encrypted object required; source
///   retained until an audited encrypting facility confirms handoff
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceMediaRef {
    pub id: String,
    pub kind: String,
    pub artifact_id: String,
    pub relative_path: String,
    pub content_hash: EvidenceContentHash,
    pub mime_type: String,
    pub byte_length: u64,
    pub file_name: Option<String>,
    pub protection: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceWindowBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceWindowMetadata {
    pub title: Option<String>,
    pub application: Option<String>,
    pub bounds: Option<EvidenceWindowBounds>,
    pub capture_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceProposedTask {
    pub title: String,
    pub summary: String,
    pub acceptance_criteria: Vec<String>,
    pub steps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceRedaction {
    pub target: String,
    pub target_id: String,
    pub reason: String,
    pub redacted_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceReview {
    pub privacy_reviewed: bool,
    pub reviewed_at: Option<String>,
    pub reviewer_user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceSignature {
    pub algorithm: String,
    pub key_id: String,
    pub hex: String,
    pub signed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceManifest {
    pub schema_version: String,
    pub bundle_id: String,
    pub exported_at: String,
    pub time_range: EvidenceTimeRange,
    pub source: EvidenceSourceIdentity,
    pub transcript_segments: Vec<EvidenceTranscriptSegment>,
    pub media: Vec<EvidenceMediaRef>,
    pub windows: Vec<EvidenceWindowMetadata>,
    pub proposed_task: EvidenceProposedTask,
    pub privacy_labels: Vec<String>,
    pub redactions: Vec<EvidenceRedaction>,
    pub review: EvidenceReview,
    pub signature: Option<EvidenceSignature>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceIssue {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceValidationReport {
    pub ok: bool,
    pub errors: Vec<EvidenceIssue>,
    pub bundle_id: Option<String>,
}

impl EvidenceValidationReport {
    pub fn has_code(&self, code: &str) -> bool {
        self.errors.iter().any(|e| e.code == code)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceTaskDraft {
    pub source_bundle_id: String,
    pub title: String,
    pub summary: String,
    pub acceptance_criteria: Vec<String>,
    pub steps: Vec<String>,
    pub selected_media_ids: Vec<String>,
    pub selected_transcript_ids: Vec<String>,
    pub scope: String,
    pub project_workspace: Option<String>,
    pub provider: Option<String>,
}

/// Command-issued review receipt. Query params alone are not authoritative.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceReviewReceipt {
    pub receipt_id: String,
    pub bundle_id: String,
    pub reviewed_at: String,
    pub draft: EvidenceTaskDraft,
    pub signature: EvidenceSignature,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceExportResult {
    pub bundle_id: String,
    pub bundle_dir: String,
    pub manifest_path: String,
    pub artifacts_dir: String,
    pub manifest: EvidenceManifest,
    pub review_required: bool,
    /// Present when large media needs a separate encrypted-object facility.
    pub encrypted_object_handoff_required: bool,
}

/// Public device identity. This is safe to persist and to send to Portal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DevicePublicIdentity {
    pub device_id: String,
    pub device_key_id: String,
}

/// Signing material loaded into memory from OS-protected storage. The key is
/// never serialized into IPC, logs, manifests, or project/config files.
#[derive(Clone)]
pub struct DeviceIdentity {
    pub device_id: String,
    pub device_key_id: String,
    pub hmac_key_hex: String,
}

impl std::fmt::Debug for DeviceIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceIdentity")
            .field("device_id", &self.device_id)
            .field("device_key_id", &self.device_key_id)
            .field("hmac_key_hex", &"[redacted]")
            .finish()
    }
}

pub trait SignerProvider: Send + Sync {
    fn identity(&self) -> DeviceIdentity;
    fn sign_hex(&self, payload: &[u8]) -> Result<String, String>;
}

#[derive(Debug, Clone)]
pub struct InjectedHmacSigner {
    pub identity: DeviceIdentity,
}

impl SignerProvider for InjectedHmacSigner {
    fn identity(&self) -> DeviceIdentity {
        self.identity.clone()
    }

    fn sign_hex(&self, payload: &[u8]) -> Result<String, String> {
        let key = crate::evidence::crypto::hex_decode(&self.identity.hmac_key_hex)?;
        Ok(crate::evidence::crypto::hmac_sha256_hex(&key, payload))
    }
}

#[derive(Debug, Clone, Default)]
pub struct ValidationContext {
    pub trusted_key_ids: Vec<String>,
    pub imported_bundle_ids: Vec<String>,
    pub device_keys: HashMap<String, String>,
    pub artifacts: HashMap<String, Vec<u8>>,
    pub media_root: Option<std::path::PathBuf>,
    /// When false (export path), skip duplicate_import checks.
    pub check_duplicate_import: bool,
}
