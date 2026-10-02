use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use super::crypto::hash_file;
use super::limits::{
    EVIDENCE_BUNDLE_SCHEMA_VERSION, MAX_ARTIFACT_BYTES, PROTECTION_ENCRYPTED_HANDOFF,
    PROTECTION_NONE,
};
use super::sign::sign_manifest_with_provider;
use super::types::*;
use super::validate::{contained_join, validate_evidence_bundle};

pub const RECORDING_VIDEO_FILENAME: &str = "recording.webm";
pub const RECORDING_THUMBNAIL_FILENAME: &str = "thumbnail.jpg";
const LARGE_MEDIA_BYTES: u64 = 256 * 1024;

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent", path.display()))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("evidence");
    let temp = parent.join(format!(".{file_name}.{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = std::fs::File::create(&temp)
            .map_err(|e| format!("create temporary {}: {e}", temp.display()))?;
        file.write_all(bytes)
            .map_err(|e| format!("write temporary {}: {e}", temp.display()))?;
        file.sync_all()
            .map_err(|e| format!("sync temporary {}: {e}", temp.display()))?;
        std::fs::rename(&temp, path)
            .map_err(|e| format!("replace {} atomically: {e}", path.display()))
    })();
    match result {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            // Another exporter may have installed the same bytes between our
            // existence check and rename. Treat that exact result as the
            // idempotent success case; conflicting bytes remain an error.
            if path.is_file() && std::fs::read(path).ok().as_deref() == Some(bytes) {
                Ok(())
            } else {
                Err(error)
            }
        }
    }
}

fn copy_media_atomic(source: &Path, target: &Path, media: &EvidenceMediaRef) -> Result<(), String> {
    if let Ok(metadata) = std::fs::metadata(target) {
        if metadata.len() > MAX_ARTIFACT_BYTES {
            return Err(format!("artifact {} exceeds size bound", media.artifact_id));
        }
        let (length, hash) = hash_file(target)?;
        if length == media.byte_length && hash == media.content_hash.hex {
            return Ok(());
        }
        return Err(format!(
            "artifact {} already exists with different content",
            media.artifact_id
        ));
    }

    let parent = target
        .parent()
        .ok_or_else(|| format!("{} has no parent", target.display()))?;
    let temp = parent.join(format!(
        ".{}.{}.tmp",
        media.artifact_id,
        uuid::Uuid::new_v4()
    ));
    let result = (|| {
        let mut input = std::fs::File::open(source)
            .map_err(|e| format!("open media {}: {e}", source.display()))?;
        let mut output =
            std::fs::File::create(&temp).map_err(|e| format!("create temporary artifact: {e}"))?;
        let mut buffer = [0u8; 64 * 1024];
        let mut copied = 0u64;
        loop {
            let read = input
                .read(&mut buffer)
                .map_err(|e| format!("read media {}: {e}", source.display()))?;
            if read == 0 {
                break;
            }
            copied = copied.saturating_add(read as u64);
            if copied > MAX_ARTIFACT_BYTES {
                return Err(format!("artifact {} exceeds size bound", media.artifact_id));
            }
            output
                .write_all(&buffer[..read])
                .map_err(|e| format!("write temporary artifact: {e}"))?;
        }
        output
            .sync_all()
            .map_err(|e| format!("sync temporary artifact: {e}"))?;
        let (length, hash) = hash_file(&temp)?;
        if length != media.byte_length || hash != media.content_hash.hex {
            return Err(format!("source media {} changed during export", media.id));
        }
        std::fs::rename(&temp, target)
            .map_err(|e| format!("install artifact {}: {e}", media.artifact_id))
    })();
    match result {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            if target.is_file() {
                let (length, hash) = hash_file(target)?;
                if length == media.byte_length && hash == media.content_hash.hex {
                    return Ok(());
                }
            }
            Err(error)
        }
    }
}

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
    uuid::Uuid::parse_str(&req.bundle_id).map_err(|_| "bundle_id must be a UUID".to_string())?;
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

    // No audited encryption/object-store provider is available in this process.
    // Never claim a handoff succeeded or write a manifest that points at absent
    // ciphertext. The source recording remains untouched for a later handoff.
    if signed
        .media
        .iter()
        .any(|media| media.protection == PROTECTION_ENCRYPTED_HANDOFF)
    {
        return Err(
            "encrypted-object handoff is unavailable; refusing to export large media".into(),
        );
    }

    let bundle_dir = req.evidence_root.join(&req.bundle_id);
    let artifacts_dir = bundle_dir.join("artifacts");
    std::fs::create_dir_all(&artifacts_dir).map_err(|e| format!("create evidence dir: {e}"))?;
    let manifest_path = bundle_dir.join("manifest.json");

    for media in &signed.media {
        let source = contained_join(&req.recording_folder, &media.relative_path)?;
        if !source.is_file() {
            return Err(format!("media {} is missing", media.id));
        }
        let target = artifacts_dir.join(&media.artifact_id);
        copy_media_atomic(&source, &target, media)?;
    }

    let manifest_json = serde_json::to_string_pretty(&signed).map_err(|e| e.to_string())?;
    if manifest_path.exists() {
        let existing_raw = std::fs::read_to_string(&manifest_path)
            .map_err(|e| format!("read existing manifest: {e}"))?;
        let existing: EvidenceManifest = serde_json::from_str(&existing_raw)
            .map_err(|e| format!("existing manifest is corrupt: {e}"))?;
        if existing != signed {
            return Err(format!(
                "bundle {} already exists with different content",
                req.bundle_id
            ));
        }
    } else {
        write_atomic(&manifest_path, manifest_json.as_bytes())?;
    }

    // Retain source media in the recording folder. Successful exports contain
    // only locally copied, unencrypted refs; no handoff was performed.

    Ok(EvidenceExportResult {
        bundle_id: req.bundle_id,
        bundle_dir: bundle_dir.to_string_lossy().into_owned(),
        manifest_path: manifest_path.to_string_lossy().into_owned(),
        artifacts_dir: artifacts_dir.to_string_lossy().into_owned(),
        manifest: signed,
        review_required: true,
        encrypted_object_handoff_required: false,
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
        );

        let error = result.unwrap_err();
        assert!(error.contains("encrypted-object handoff"));
        assert_eq!(
            std::fs::read(media_dir.join(RECORDING_VIDEO_FILENAME)).unwrap(),
            large
        );
        let _ = std::fs::remove_dir_all(media_dir);
        let _ = std::fs::remove_dir_all(evidence_root);
    }

    #[test]
    fn export_copies_small_media_and_repeats_idempotently() {
        let media_dir =
            std::env::temp_dir().join(format!("ev-media-small-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&media_dir).unwrap();
        std::fs::write(media_dir.join(RECORDING_VIDEO_FILENAME), b"abc").unwrap();
        let evidence_root =
            std::env::temp_dir().join(format!("ev-out-small-{}", uuid::Uuid::new_v4()));
        let identity = test_identity();
        let signer = InjectedHmacSigner {
            identity: identity.clone(),
        };
        let request = ExportRequest {
            recording_id: "rec-1".into(),
            recording_folder: media_dir.clone(),
            windows: vec![],
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
        };
        let first = export_evidence_bundle(request.clone(), &signer).unwrap();
        let artifact_path = PathBuf::from(&first.artifacts_dir).join("art-recording");
        assert_eq!(std::fs::read(artifact_path).unwrap(), b"abc");
        let second = export_evidence_bundle(request, &signer).unwrap();
        assert_eq!(first.manifest, second.manifest);
        let _ = std::fs::remove_dir_all(media_dir);
        let _ = std::fs::remove_dir_all(evidence_root);
    }
}
