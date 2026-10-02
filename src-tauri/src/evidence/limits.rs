pub use super::types::EVIDENCE_BUNDLE_SCHEMA_VERSION;

pub const MAX_TRANSCRIPT_SEGMENTS: usize = 200;
pub const MAX_SEGMENT_TEXT_BYTES: usize = 8192;
pub const MAX_MEDIA_REFS: usize = 32;
pub const MAX_WINDOWS: usize = 16;
pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_SUMMARY_BYTES: usize = 8192;
pub const MAX_ACCEPTANCE_CRITERIA: usize = 20;
pub const MAX_CRITERION_CHARS: usize = 500;
pub const MAX_STEPS: usize = 50;
pub const MAX_STEP_CHARS: usize = 500;
pub const MAX_REDACTIONS: usize = 100;
pub const MAX_PRIVACY_LABELS: usize = 8;
pub const MAX_BUNDLE_ID_CHARS: usize = 64;
pub const MAX_TIME_ZONE_CHARS: usize = 64;
pub const MAX_PATH_CHARS: usize = 512;
pub const MAX_CAPTURE_DURATION_MS: i64 = 7 * 24 * 60 * 60 * 1000;
pub const MAX_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;
pub const REDACTED_TEXT: &str = "[REDACTED]";
pub const PRIVACY_LABELS: &[&str] = &[
    "contains_pii",
    "contains_credentials",
    "contains_customer_data",
    "internal_only",
    "public_safe",
];
pub const ALLOWED_HASH_ALG: &str = "sha256";
pub const ALLOWED_SIG_ALG: &str = "hmac-sha256";
pub const PROTECTION_NONE: &str = "none";
pub const PROTECTION_ENCRYPTED_HANDOFF: &str = "encryptedObjectHandoffRequired";
