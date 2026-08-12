//! Tauri EvidenceBundle v1 commands (plan path: evidence_bundle.rs).
//!
//! Signing secrets are injected for the process session and never written to disk.
//! Import idempotency is separate from export.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;

use serde::Deserialize;
use tauri::{AppHandle, Manager, State};

use crate::commands::recordings::{
    recordings_root, PersistedCaptureTarget, RECORDING_METADATA_FILENAME,
};
use crate::evidence::crypto::{hex_decode, hmac_sha256_hex};
use crate::evidence::export::{empty_proposed_task, ExportRequest};
use crate::evidence::index::{load_import_index, register_imported_bundle_id};
use crate::evidence::limits::ALLOWED_SIG_ALG;
use crate::evidence::sign::sign_manifest_with_provider;
use crate::evidence::types::*;
use crate::evidence::validate::{
    create_task_draft_with_selection, validate_evidence_bundle as validate_manifest,
};

#[derive(Default)]
pub struct SessionSignerState {
    inner: Mutex<Option<DeviceIdentity>>,
}

fn evidence_root(app: &AppHandle) -> Result<PathBuf, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir failed: {e}"))?
        .join("evidence");
    std::fs::create_dir_all(&root).map_err(|e| format!("create evidence root: {e}"))?;
    Ok(root)
}

fn iso8601_now() -> String {
    let now = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = now.as_secs();
    let millis = now.subsec_millis();
    let days = total_secs / 86400;
    let secs_of_day = total_secs % 86400;
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;
    let (year, month, day) = civil_from_days(days as i64);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

fn window_from_target(target: &PersistedCaptureTarget) -> EvidenceWindowMetadata {
    match target {
        PersistedCaptureTarget::Screen => EvidenceWindowMetadata {
            title: None,
            application: None,
            bounds: None,
            capture_mode: "screen".into(),
        },
        PersistedCaptureTarget::Monitor { label } => EvidenceWindowMetadata {
            title: None,
            application: Some(label.clone()),
            bounds: None,
            capture_mode: "monitor".into(),
        },
        PersistedCaptureTarget::Window { title } => EvidenceWindowMetadata {
            title: Some(title.clone()),
            application: None,
            bounds: None,
            capture_mode: "window".into(),
        },
        PersistedCaptureTarget::Region { width, height } => EvidenceWindowMetadata {
            title: None,
            application: None,
            bounds: Some(EvidenceWindowBounds {
                x: 0,
                y: 0,
                width: *width,
                height: *height,
            }),
            capture_mode: "region".into(),
        },
    }
}

fn load_recording_meta(
    app: &AppHandle,
    recording_id: &str,
) -> Result<(PathBuf, crate::commands::recordings::RecordingMeta), String> {
    if recording_id.contains("..") || recording_id.contains('/') || recording_id.contains('\\') {
        return Err("recording_id must be a plain id".into());
    }
    let folder = recordings_root(app)?.join(recording_id);
    let raw = std::fs::read_to_string(folder.join(RECORDING_METADATA_FILENAME))
        .map_err(|e| format!("read metadata: {e}"))?;
    let meta = serde_json::from_str(&raw).map_err(|e| format!("parse metadata: {e}"))?;
    Ok((folder, meta))
}

fn load_public_identity(path: &std::path::Path) -> Result<Option<DevicePublicIdentity>, String> {
    match std::fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw)
            .map(Some)
            .map_err(|e| format!("public evidence identity corrupt: {e}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("read public evidence identity: {error}")),
    }
}

fn write_public_identity_atomic(
    path: &std::path::Path,
    identity: &DevicePublicIdentity,
) -> Result<(), String> {
    use std::io::Write;

    let parent = path
        .parent()
        .ok_or_else(|| "public evidence identity has no parent".to_string())?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name().unwrap().to_string_lossy(),
        uuid::Uuid::new_v4()
    ));
    let result = (|| {
        let mut file = std::fs::File::create(&temporary)
            .map_err(|e| format!("create public evidence identity: {e}"))?;
        let json = serde_json::to_vec_pretty(identity).map_err(|e| e.to_string())?;
        file.write_all(&json)
            .map_err(|e| format!("write public evidence identity: {e}"))?;
        file.sync_all()
            .map_err(|e| format!("sync public evidence identity: {e}"))?;
        std::fs::rename(&temporary, path)
            .map_err(|e| format!("install public evidence identity: {e}"))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

/// Session cache over the OS-protected enrolled signer. The protected record
/// is authoritative; the adjacent public file contains no signing material.
fn session_identity(app: &AppHandle, state: &SessionSignerState) -> Result<DeviceIdentity, String> {
    let mut guard = state.inner.lock().map_err(|e| e.to_string())?;
    if let Some(existing) = guard.as_ref() {
        return Ok(existing.clone());
    }
    let root = evidence_root(app)?;
    let public_path = root.join("device_public.json");
    let public = load_public_identity(&public_path)?;
    let identity = crate::evidence::key_store::load_or_create(&root, public.as_ref())?;
    if public.is_none() {
        write_public_identity_atomic(
            &public_path,
            &DevicePublicIdentity {
                device_id: identity.device_id.clone(),
                device_key_id: identity.device_key_id.clone(),
            },
        )?;
    }
    *guard = Some(identity.clone());
    Ok(identity)
}

fn build_request(
    app: &AppHandle,
    identity: DeviceIdentity,
    recording_id: String,
    proposed_task: EvidenceProposedTask,
    transcript_segments: Vec<EvidenceTranscriptSegment>,
    time_zone: Option<String>,
    privacy_labels: Vec<String>,
    redactions: Vec<EvidenceRedaction>,
    privacy_reviewed: bool,
    reviewer_user_id: Option<String>,
) -> Result<ExportRequest, String> {
    let (folder, meta) = load_recording_meta(app, &recording_id)?;
    let root = evidence_root(app)?;
    let now = iso8601_now();
    Ok(ExportRequest {
        recording_id,
        recording_folder: folder,
        windows: vec![window_from_target(&meta.capture_target)],
        transcript_segments,
        proposed_task,
        privacy_labels,
        redactions,
        time_zone: time_zone.unwrap_or_else(|| "UTC".into()),
        started_at: meta.created_at.clone(),
        ended_at: now.clone(),
        exported_at: now.clone(),
        bundle_id: uuid::Uuid::new_v4().to_string(),
        signed_at: now.clone(),
        identity,
        user_id: None,
        display_name: None,
        privacy_reviewed,
        reviewed_at: if privacy_reviewed { Some(now) } else { None },
        reviewer_user_id,
        evidence_root: root,
    })
}

/// Return the enrolled public device identity. The HMAC secret stays in
/// OS-protected storage and is never returned over IPC.
#[tauri::command]
pub async fn get_evidence_device_identity(
    app: AppHandle,
    signer: State<'_, SessionSignerState>,
) -> Result<DevicePublicIdentity, String> {
    let identity = session_identity(&app, &signer)?;
    Ok(DevicePublicIdentity {
        device_id: identity.device_id,
        device_key_id: identity.device_key_id,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompleteEvidenceEnrollmentRequest {
    pub device_id: String,
    pub device_key_id: String,
    /// One-time server-issued secret returned by Portal host enrollment.
    pub enrollment_secret: String,
}

/// Persist Portal's one-time enrollment secret in OS-protected storage and
/// make it the signer for subsequent exports. The request must name the
/// already-created device identity; it cannot silently replace that identity.
#[tauri::command]
pub async fn complete_evidence_enrollment(
    app: AppHandle,
    signer: State<'_, SessionSignerState>,
    request: CompleteEvidenceEnrollmentRequest,
) -> Result<DevicePublicIdentity, String> {
    let current = session_identity(&app, &signer)?;
    if current.device_id != request.device_id || current.device_key_id != request.device_key_id {
        return Err("Portal enrollment identity does not match this device".into());
    }
    let public = DevicePublicIdentity {
        device_id: current.device_id,
        device_key_id: current.device_key_id,
    };
    let root = evidence_root(&app)?;
    let enrolled = crate::evidence::key_store::install_enrollment_secret(
        &root,
        &public,
        &request.enrollment_secret,
    )?;
    let mut guard = signer.inner.lock().map_err(|e| e.to_string())?;
    *guard = Some(enrolled);
    Ok(public)
}

fn review_receipt_payload(receipt: &EvidenceReviewReceipt) -> Result<String, String> {
    let mut value = serde_json::to_value(receipt).map_err(|e| e.to_string())?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| "review receipt must be an object".to_string())?;
    object.insert("signature".into(), serde_json::Value::Null);
    crate::evidence::canonical::canonical_json(&value)
}

fn verify_review_receipt(
    receipt: &EvidenceReviewReceipt,
    identity: &DeviceIdentity,
) -> Result<(), String> {
    if receipt.draft.scope != "personal" && receipt.draft.scope != "managed" {
        return Err("review receipt scope must be personal or managed".into());
    }
    if receipt.draft.source_bundle_id != receipt.bundle_id {
        return Err("review receipt bundle id does not match the draft".into());
    }
    if receipt.signature.algorithm != ALLOWED_SIG_ALG
        || receipt.signature.key_id != identity.device_key_id
    {
        return Err("review receipt signer is not trusted".into());
    }
    let key = hex_decode(&identity.hmac_key_hex)?;
    let expected = hmac_sha256_hex(&key, review_receipt_payload(receipt)?.as_bytes());
    if expected != receipt.signature.hex {
        return Err("review receipt signature is invalid".into());
    }
    Ok(())
}

fn write_receipt_atomic(
    path: &std::path::Path,
    receipt: &EvidenceReviewReceipt,
) -> Result<(), String> {
    use std::io::Write;
    if path.exists() {
        return Err("review receipt already exists".into());
    }
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent", path.display()))?;
    let temp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name().unwrap().to_string_lossy(),
        uuid::Uuid::new_v4()
    ));
    let result = (|| {
        let json = serde_json::to_vec_pretty(receipt).map_err(|e| e.to_string())?;
        let mut file =
            std::fs::File::create(&temp).map_err(|e| format!("create temporary receipt: {e}"))?;
        file.write_all(&json)
            .map_err(|e| format!("write temporary receipt: {e}"))?;
        file.sync_all()
            .map_err(|e| format!("sync temporary receipt: {e}"))?;
        if path.exists() {
            return Err("review receipt already exists".into());
        }
        std::fs::rename(&temp, path).map_err(|e| format!("install receipt atomically: {e}"))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

#[tauri::command]
pub async fn preview_evidence_bundle(
    app: AppHandle,
    signer: State<'_, SessionSignerState>,
    recording_id: String,
) -> Result<EvidenceManifest, String> {
    let identity = session_identity(&app, &signer)?;
    let req = build_request(
        &app,
        identity,
        recording_id,
        empty_proposed_task(),
        vec![],
        None,
        vec!["internal_only".into()],
        vec![],
        false,
        None,
    )?;
    crate::evidence::preview_evidence_bundle(&req)
}

#[tauri::command]
pub async fn export_evidence_bundle(
    app: AppHandle,
    signer: State<'_, SessionSignerState>,
    recording_id: String,
    proposed_task: EvidenceProposedTask,
    transcript_segments: Option<Vec<EvidenceTranscriptSegment>>,
    time_zone: Option<String>,
    privacy_labels: Option<Vec<String>>,
    redactions: Option<Vec<EvidenceRedaction>>,
    privacy_reviewed: Option<bool>,
    reviewer_user_id: Option<String>,
) -> Result<EvidenceExportResult, String> {
    let identity = session_identity(&app, &signer)?;
    let req = build_request(
        &app,
        identity.clone(),
        recording_id,
        proposed_task,
        transcript_segments.unwrap_or_default(),
        time_zone,
        privacy_labels.unwrap_or_else(|| vec!["internal_only".into()]),
        redactions.unwrap_or_default(),
        privacy_reviewed.unwrap_or(false),
        reviewer_user_id,
    )?;
    let provider = InjectedHmacSigner { identity };
    // Intentionally does NOT write the import index.
    crate::evidence::export_evidence_bundle(req, &provider)
}

#[tauri::command]
pub async fn validate_evidence_bundle(
    app: AppHandle,
    signer: State<'_, SessionSignerState>,
    bundle: EvidenceManifest,
    recording_id: Option<String>,
    check_duplicate_import: Option<bool>,
) -> Result<EvidenceValidationReport, String> {
    let identity = session_identity(&app, &signer)?;
    let root = evidence_root(&app)?;
    let imported = if check_duplicate_import.unwrap_or(false) {
        load_import_index(&root)?.bundle_ids
    } else {
        vec![]
    };
    let media_root = match recording_id {
        Some(id) => Some(load_recording_meta(&app, &id)?.0),
        None => None,
    };
    Ok(validate_manifest(
        &bundle,
        &ValidationContext {
            trusted_key_ids: vec![identity.device_key_id.clone()],
            imported_bundle_ids: imported,
            device_keys: [(identity.device_key_id, identity.hmac_key_hex)]
                .into_iter()
                .collect(),
            artifacts: HashMap::new(),
            media_root,
            check_duplicate_import: check_duplicate_import.unwrap_or(false),
        },
    ))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmEvidenceReviewRequest {
    pub bundle: EvidenceManifest,
    pub recording_id: String,
    pub scope: String,
    pub project_workspace: Option<String>,
    pub provider: Option<String>,
    pub selected_media_ids: Option<Vec<String>>,
    pub selected_transcript_ids: Option<Vec<String>>,
    pub proposed_task: EvidenceProposedTask,
    pub reviewer_user_id: Option<String>,
}

/// Authoritative review-before-create. Returns a signed receipt; query params are not enough.
#[tauri::command]
pub async fn confirm_evidence_review(
    app: AppHandle,
    signer: State<'_, SessionSignerState>,
    request: ConfirmEvidenceReviewRequest,
) -> Result<EvidenceReviewReceipt, String> {
    if request.scope != "personal" && request.scope != "managed" {
        return Err("scope must be personal or managed".into());
    }
    let identity = session_identity(&app, &signer)?;
    let (folder, _) = load_recording_meta(&app, &request.recording_id)?;
    let incoming_bundle = request.bundle;
    let incoming_report = validate_manifest(
        &incoming_bundle,
        &ValidationContext {
            trusted_key_ids: vec![identity.device_key_id.clone()],
            imported_bundle_ids: vec![],
            device_keys: [(
                identity.device_key_id.clone(),
                identity.hmac_key_hex.clone(),
            )]
            .into_iter()
            .collect(),
            artifacts: HashMap::new(),
            media_root: Some(folder.clone()),
            check_duplicate_import: false,
        },
    );
    if !incoming_report.ok {
        return Err(incoming_report
            .errors
            .into_iter()
            .map(|e| format!("{}: {}", e.code, e.message))
            .collect::<Vec<_>>()
            .join("; "));
    }
    if incoming_bundle.source.device_id != identity.device_id
        || incoming_bundle.source.device_key_id != identity.device_key_id
    {
        return Err("review requires a bundle signed by this authenticated device".into());
    }
    let mut bundle = incoming_bundle;
    bundle.proposed_task = request.proposed_task;
    bundle.review = EvidenceReview {
        privacy_reviewed: true,
        reviewed_at: Some(iso8601_now()),
        reviewer_user_id: request.reviewer_user_id,
    };
    let provider = InjectedHmacSigner {
        identity: identity.clone(),
    };
    let signed = sign_manifest_with_provider(&bundle, &provider, &iso8601_now())?;
    let ctx = ValidationContext {
        trusted_key_ids: vec![identity.device_key_id.clone()],
        imported_bundle_ids: vec![],
        device_keys: [(
            identity.device_key_id.clone(),
            identity.hmac_key_hex.clone(),
        )]
        .into_iter()
        .collect(),
        artifacts: HashMap::new(),
        media_root: Some(folder),
        check_duplicate_import: false,
    };
    let (draft, report) = create_task_draft_with_selection(
        &signed,
        &ctx,
        &request.scope,
        request.project_workspace,
        request.provider,
        request.selected_media_ids.as_deref(),
        request.selected_transcript_ids.as_deref(),
    );
    if !report.ok {
        return Err(report
            .errors
            .into_iter()
            .map(|e| format!("{}: {}", e.code, e.message))
            .collect::<Vec<_>>()
            .join("; "));
    }
    let draft = draft.ok_or_else(|| "draft missing".to_string())?;
    let receipt_id = uuid::Uuid::new_v4().to_string();
    let reviewed_at = iso8601_now();
    let mut receipt = EvidenceReviewReceipt {
        receipt_id: receipt_id.clone(),
        bundle_id: signed.bundle_id.clone(),
        reviewed_at: reviewed_at.clone(),
        draft,
        signature: EvidenceSignature {
            algorithm: "hmac-sha256".into(),
            key_id: identity.device_key_id.clone(),
            hex: String::new(),
            signed_at: reviewed_at,
        },
    };
    let canonical = review_receipt_payload(&receipt)?;
    receipt.signature.hex = provider.sign_hex(canonical.as_bytes())?;

    let root = evidence_root(&app)?;
    let receipt_dir = root.join("receipts");
    std::fs::create_dir_all(&receipt_dir).map_err(|e| e.to_string())?;
    write_receipt_atomic(&receipt_dir.join(format!("{receipt_id}.json")), &receipt)?;
    Ok(receipt)
}

#[tauri::command]
pub async fn get_evidence_review_receipt(
    app: AppHandle,
    signer: State<'_, SessionSignerState>,
    receipt_id: String,
) -> Result<EvidenceReviewReceipt, String> {
    if uuid::Uuid::parse_str(&receipt_id).is_err() {
        return Err("invalid receipt id".into());
    }
    let identity = session_identity(&app, &signer)?;
    let path = evidence_root(&app)?
        .join("receipts")
        .join(format!("{receipt_id}.json"));
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("receipt not found: {e}"))?;
    let receipt: EvidenceReviewReceipt =
        serde_json::from_str(&raw).map_err(|e| format!("receipt corrupt: {e}"))?;
    if receipt.receipt_id != receipt_id {
        return Err("receipt id does not match its filename".into());
    }
    verify_review_receipt(&receipt, &identity)?;
    Ok(receipt)
}

/// Explicit import path — this is where duplicate/idempotency is enforced.
#[tauri::command]
pub async fn import_evidence_bundle(
    app: AppHandle,
    signer: State<'_, SessionSignerState>,
    bundle: EvidenceManifest,
    recording_id: String,
) -> Result<EvidenceValidationReport, String> {
    let identity = session_identity(&app, &signer)?;
    let root = evidence_root(&app)?;
    let imported = load_import_index(&root)?;
    let folder = load_recording_meta(&app, &recording_id)?.0;
    let mut report = validate_manifest(
        &bundle,
        &ValidationContext {
            trusted_key_ids: vec![identity.device_key_id.clone()],
            imported_bundle_ids: imported.bundle_ids,
            device_keys: [(identity.device_key_id, identity.hmac_key_hex)]
                .into_iter()
                .collect(),
            artifacts: HashMap::new(),
            media_root: Some(folder),
            check_duplicate_import: true,
        },
    );
    if report.ok {
        if !register_imported_bundle_id(&root, &bundle.bundle_id)? {
            report.ok = false;
            report.errors.push(EvidenceIssue {
                code: "duplicate_import".into(),
                message: format!("bundle {} was already imported", bundle.bundle_id),
            });
        }
    }
    Ok(report)
}

#[cfg(test)]
mod evidence_bundle_tests {
    use super::*;
    use crate::evidence::types::{InjectedHmacSigner, SignerProvider};

    fn receipt(identity: &DeviceIdentity) -> EvidenceReviewReceipt {
        let reviewed_at = "2026-08-11T19:00:00.000Z".to_string();
        let mut receipt = EvidenceReviewReceipt {
            receipt_id: "11111111-1111-4111-8111-111111111111".into(),
            bundle_id: "22222222-2222-4222-8222-222222222222".into(),
            reviewed_at: reviewed_at.clone(),
            draft: EvidenceTaskDraft {
                source_bundle_id: "22222222-2222-4222-8222-222222222222".into(),
                title: "Reviewed task".into(),
                summary: "Summary".into(),
                acceptance_criteria: vec![],
                steps: vec![],
                selected_media_ids: vec!["media-1".into()],
                selected_transcript_ids: vec![],
                scope: "personal".into(),
                project_workspace: None,
                provider: None,
            },
            signature: EvidenceSignature {
                algorithm: ALLOWED_SIG_ALG.into(),
                key_id: identity.device_key_id.clone(),
                hex: String::new(),
                signed_at: reviewed_at,
            },
        };
        let signer = InjectedHmacSigner {
            identity: identity.clone(),
        };
        receipt.signature.hex = signer
            .sign_hex(review_receipt_payload(&receipt).unwrap().as_bytes())
            .unwrap();
        receipt
    }

    #[test]
    fn reviewed_receipt_is_authenticated_and_tamper_evident() {
        let identity = crate::evidence::test_support::test_identity();
        let receipt = receipt(&identity);
        assert!(verify_review_receipt(&receipt, &identity).is_ok());

        let mut tampered = receipt.clone();
        tampered.draft.title = "Changed after review".into();
        assert!(verify_review_receipt(&tampered, &identity).is_err());
        assert!(
            verify_review_receipt(&receipt, &crate::evidence::test_support::alt_identity())
                .is_err()
        );
    }

    #[test]
    fn receipt_scope_and_bundle_linkage_are_fail_closed() {
        let identity = crate::evidence::test_support::test_identity();
        let mut invalid_scope = receipt(&identity);
        invalid_scope.draft.scope = "shared".into();
        assert!(verify_review_receipt(&invalid_scope, &identity).is_err());

        let mut invalid_link = receipt(&identity);
        invalid_link.draft.source_bundle_id = "33333333-3333-4333-8333-333333333333".into();
        assert!(verify_review_receipt(&invalid_link, &identity).is_err());
    }

    #[test]
    fn receipt_file_is_not_overwritten() {
        let root = std::env::temp_dir().join(format!("ev-receipt-{}", uuid::Uuid::new_v4()));
        let path = root.join("receipts").join("receipt.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let identity = crate::evidence::test_support::test_identity();
        let receipt = receipt(&identity);
        write_receipt_atomic(&path, &receipt).unwrap();
        assert!(write_receipt_atomic(&path, &receipt).is_err());
        let _ = std::fs::remove_dir_all(root);
    }
}
