use std::path::{Path, PathBuf};

use super::crypto::hash_file;
use super::limits::{
    EVIDENCE_BUNDLE_SCHEMA_VERSION, PROTECTION_ENCRYPTED_HANDOFF, PROTECTION_NONE,
};
use super::sign::sign_manifest_with_provider;
use super::types::*;
use super::validate::{contained_join, validate_evidence_bundle};

pub const RECORDING_VIDEO_FILENAME: &str = "recording.webm";
pub const RECORDING_THUMBNAIL_FILENAME: &str = "thumbnail.jpg";
const LARGE_MEDIA_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone)]
pub struct ExportRequest {
    pub recording_id: String,
    pub recording_folder: PathBuf,
    pub windows: Vec<EvidenceWindowMetadata>,
    pub transcript_segments: Vec<EvidenceTranscriptSegment>,
    pub proposed_task: EvidenceProposedTask,
    pub privacy_labels: Vec<String>,
    pub redactions: Vec<EvidenceRedaction>,
    pub time_zone: String,
    pub started_at: String,
    pub ended_at: String,
    pub exported_at: String,
    pub bundle_id: String,
    pub signed_at: String,
    pub identity: DeviceIdentity,
    pub user_id: Option<String>,
    pub display_name: Option<String>,
    pub privacy_reviewed: bool,
    pub reviewed_at: Option<String>,
    pub reviewer_user_id: Option<String>,
    pub evidence_root: PathBuf,
}

fn media_from_file(
    id: &str,
    kind: &str,
    artifact_id: &str,
    folder: &Path,
    file_name: &str,
    mime_type: &str,
) -> Result<Option<EvidenceMediaRef>, String> {
    let path = contained_join(folder, file_name)?;
    if !path.is_file() {
        return Ok(None);
    }
    let (byte_length, hex) = hash_file(&path)?;
    let protection = if byte_length > LARGE_MEDIA_BYTES {
        PROTECTION_ENCRYPTED_HANDOFF
    } else {
        PROTECTION_NONE
    };
    Ok(Some(EvidenceMediaRef {
        id: id.into(),
        kind: kind.into(),
        artifact_id: artifact_id.into(),
        // Relative to the recording root — never leave that root.
        relative_path: file_name.into(),
        content_hash: EvidenceContentHash {
            algorithm: "sha256".into(),
            hex,
        },
        mime_type: mime_type.into(),
        byte_length,
        file_name: Some(file_name.into()),
        protection: protection.into(),
    }))
}

pub fn empty_proposed_task() -> EvidenceProposedTask {
    EvidenceProposedTask {
        title: String::new(),
        summary: String::new(),
        acceptance_criteria: vec![],
        steps: vec![],
    }
}

pub fn preview_evidence_bundle(req: &ExportRequest) -> Result<EvidenceManifest, String> {
    let mut media = Vec::new();
    if let Some(refer) = media_from_file(
        "media-recording",
        "recording",
        "art-recording",
        &req.recording_folder,
        RECORDING_VIDEO_FILENAME,
        "video/webm",
    )? {
        media.push(refer);
    }
    if let Some(refer) = media_from_file(
        "media-screenshot",
        "screenshot",
        "art-screenshot",
        &req.recording_folder,
        RECORDING_THUMBNAIL_FILENAME,
        "image/jpeg",
    )? {
        media.push(refer);
    }

    Ok(EvidenceManifest {
        schema_version: EVIDENCE_BUNDLE_SCHEMA_VERSION.into(),
        bundle_id: req.bundle_id.clone(),
        exported_at: req.exported_at.clone(),
        time_range: EvidenceTimeRange {
            started_at: req.started_at.clone(),
            ended_at: req.ended_at.clone(),
            time_zone: req.time_zone.clone(),
        },
        source: EvidenceSourceIdentity {
            device_id: req.identity.device_id.clone(),
            device_key_id: req.identity.device_key_id.clone(),
            user_id: req.user_id.clone(),
            display_name: req.display_name.clone(),
        },
        transcript_segments: req.transcript_segments.clone(),
        media,
        windows: req.windows.clone(),
        proposed_task: req.proposed_task.clone(),
        privacy_labels: req.privacy_labels.clone(),
        redactions: req.redactions.clone(),
        review: EvidenceReview {
            privacy_reviewed: req.privacy_reviewed,
            reviewed_at: req.reviewed_at.clone(),
            reviewer_user_id: req.reviewer_user_id.clone(),
        },
        signature: None,
    })
}

pub fn export_evidence_bundle(
    req: ExportRequest,
    signer: &dyn SignerProvider,
) -> Result<EvidenceExportResult, String> {
    let unsigned = preview_evidence_bundle(&req)?;
    let signed = sign_manifest_with_provider(&unsigned, signer, &req.signed_at)?;
    // Export must not consult import idempotency — a fresh export is not an import.
    let ctx = ValidationContext {
        trusted_key_ids: vec![req.identity.device_key_id.clone()],
        imported_bundle_ids: vec![],
        device_keys: [(
            req.identity.device_key_id.clone(),
            req.identity.hmac_key_hex.clone(),
        )]
        .into_iter()
        .collect(),
        artifacts: Default::default(),
        media_root: Some(req.recording_folder.clone()),
        check_duplicate_import: false,
    };
    let report = validate_evidence_bundle(&signed, &ctx);
    if !report.ok {
        return Err(format!(
            "exported bundle failed validation: {}",
            report
                .errors
                .iter()
                .map(|e| e.code.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let bundle_dir = req.evidence_root.join(&req.bundle_id);
    let artifacts_dir = bundle_dir.join("artifacts");
    std::fs::create_dir_all(&artifacts_dir).map_err(|e| format!("create evidence dir: {e}"))?;
    let manifest_path = bundle_dir.join("manifest.json");
    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&signed).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("write manifest: {e}"))?;

    let handoff = signed
        .media
        .iter()
        .any(|m| m.protection == PROTECTION_ENCRYPTED_HANDOFF);
    // Retain source media in the recording folder. Do not delete, and do not invent XOR wrapping.

    Ok(EvidenceExportResult {
        bundle_id: req.bundle_id,
        bundle_dir: bundle_dir.to_string_lossy().into_owned(),
        manifest_path: manifest_path.to_string_lossy().into_owned(),
        artifacts_dir: artifacts_dir.to_string_lossy().into_owned(),
        manifest: signed,
        review_required: true,
        encrypted_object_handoff_required: handoff,
    })
}

#[cfg(test)]
mod evidence_bundle {
    use super::*;
    use crate::evidence::test_support::test_identity;
    use crate::evidence::types::InjectedHmacSigner;

    #[test]
    fn export_keeps_large_media_as_handoff_without_copy_or_delete() {
        let media_dir = std::env::temp_dir().join(format!("ev-media-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&media_dir).unwrap();
        let large = vec![7u8; 300 * 1024];
        std::fs::write(media_dir.join(RECORDING_VIDEO_FILENAME), &large).unwrap();
        let evidence_root = std::env::temp_dir().join(format!("ev-out-{}", uuid::Uuid::new_v4()));
        let identity = test_identity();
        let signer = InjectedHmacSigner {
            identity: identity.clone(),
        };
        let result = export_evidence_bundle(
            ExportRequest {
                recording_id: "rec-1".into(),
                recording_folder: media_dir.clone(),
                windows: vec![EvidenceWindowMetadata {
                    title: None,
                    application: None,
                    bounds: None,
                    capture_mode: "screen".into(),
                }],
                transcript_segments: vec![],
                proposed_task: EvidenceProposedTask {
                    title: "T".into(),
                    summary: "S".into(),
                    acceptance_criteria: vec![],
                    steps: vec![],
                },
                privacy_labels: vec!["internal_only".into()],
                redactions: vec![],
                time_zone: "UTC".into(),
                started_at: "2026-08-11T18:55:00.000Z".into(),
                ended_at: "2026-08-11T19:00:00.000Z".into(),
                exported_at: "2026-08-11T19:00:00.000Z".into(),
                bundle_id: "11111111-1111-4111-8111-111111111111".into(),
                signed_at: "2026-08-11T19:00:01.000Z".into(),
                identity,
                user_id: None,
                display_name: None,
                privacy_reviewed: true,
                reviewed_at: Some("2026-08-11T19:00:00.000Z".into()),
                reviewer_user_id: Some("user-1".into()),
                evidence_root: evidence_root.clone(),
            },
            &signer,
        )
        .unwrap();

        assert!(result.encrypted_object_handoff_required);
        assert_eq!(
            result.manifest.media[0].protection,
            PROTECTION_ENCRYPTED_HANDOFF
        );
        assert_eq!(
            std::fs::read(media_dir.join(RECORDING_VIDEO_FILENAME)).unwrap(),
            large
        );
        let _ = std::fs::remove_dir_all(media_dir);
        let _ = std::fs::remove_dir_all(evidence_root);
    }
}
