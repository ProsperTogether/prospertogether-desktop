use std::path::{Component, Path, PathBuf};

use super::crypto::{hash_file, hex_decode, hmac_sha256_hex, sha256_hex};
use super::limits::*;
use super::sign::signing_payload;
use super::types::*;

fn issue(code: &str, message: impl Into<String>) -> EvidenceIssue {
    EvidenceIssue {
        code: code.into(),
        message: message.into(),
    }
}

pub fn is_valid_time_zone(time_zone: &str) -> bool {
    if time_zone == "UTC" || time_zone == "Etc/UTC" {
        return true;
    }
    if time_zone.is_empty() || time_zone.len() > MAX_TIME_ZONE_CHARS {
        return false;
    }
    let mut parts = time_zone.split('/');
    let Some(first) = parts.next() else {
        return false;
    };
    if first.is_empty() || !first.chars().all(|c| c.is_ascii_alphabetic() || c == '_') {
        return false;
    }
    let rest: Vec<&str> = parts.collect();
    !rest.is_empty()
        && rest.iter().all(|p| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-'))
        })
}

fn is_redacted_text(text: &str) -> bool {
    text.is_empty() || text == REDACTED_TEXT
}

fn valid_sha(hex: &str) -> bool {
    hex.len() == 64 && hex.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
}

fn looks_iso8601(value: &str) -> bool {
    let b = value.as_bytes();
    b.len() >= 20
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && value.ends_with('Z')
}

fn valid_uuid(value: &str) -> bool {
    let b = value.as_bytes();
    if b.len() != 36 {
        return false;
    }
    for (i, c) in b.iter().enumerate() {
        match i {
            8 | 13 | 18 | 23 => {
                if *c != b'-' {
                    return false;
                }
            }
            _ => {
                if !c.is_ascii_hexdigit() {
                    return false;
                }
            }
        }
    }
    true
}

/// Reject absolute paths, `..`, and backslashes. Relative names only.
pub fn is_safe_relative_path(path: &str) -> bool {
    if path.is_empty()
        || path.len() > 512
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.contains('\\')
        || path.contains('\0')
    {
        return false;
    }
    let p = Path::new(path);
    !p.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    })
}

/// Join under root only when the resolved path stays inside root.
pub fn contained_join(root: &Path, relative: &str) -> Result<PathBuf, String> {
    if !is_safe_relative_path(relative) {
        return Err(format!("unsafe relative path {relative}"));
    }
    let root = root
        .canonicalize()
        .map_err(|e| format!("canonicalize root {}: {e}", root.display()))?;
    let joined = root.join(relative);
    let canon = if joined.exists() {
        joined
            .canonicalize()
            .map_err(|e| format!("canonicalize {}: {e}", joined.display()))?
    } else {
        // Parent must exist and stay under root for not-yet-written paths.
        let parent = joined.parent().unwrap_or(&root);
        let parent = parent
            .canonicalize()
            .map_err(|e| format!("canonicalize parent {}: {e}", parent.display()))?;
        if !parent.starts_with(&root) {
            return Err("path escapes recording root".into());
        }
        return Ok(joined);
    };
    if !canon.starts_with(&root) {
        return Err("path escapes recording root".into());
    }
    Ok(canon)
}

fn validate_structure(manifest: &EvidenceManifest) -> Vec<EvidenceIssue> {
    let mut errors = Vec::new();
    if manifest.schema_version != EVIDENCE_BUNDLE_SCHEMA_VERSION {
        errors.push(issue(
            "unsupported_schema_version",
            format!(
                "schema_version {} is not supported",
                manifest.schema_version
            ),
        ));
    }
    if manifest.bundle_id.is_empty() || manifest.bundle_id.len() > MAX_BUNDLE_ID_CHARS {
        errors.push(issue("bound_exceeded", "bundle_id is missing or too long"));
    } else if !valid_uuid(&manifest.bundle_id) {
        errors.push(issue("invalid_bundle", "bundle_id must be a UUID"));
    }
    if !is_valid_time_zone(&manifest.time_range.time_zone) {
        errors.push(issue(
            "invalid_timezone",
            format!(
                "time_zone must be UTC or an IANA name, got {:?}",
                manifest.time_range.time_zone
            ),
        ));
    }
    if !looks_iso8601(&manifest.time_range.started_at)
        || !looks_iso8601(&manifest.time_range.ended_at)
        || !looks_iso8601(&manifest.exported_at)
    {
        errors.push(issue(
            "invalid_bundle",
            "timestamps must be ISO-8601 UTC ending in Z",
        ));
    } else if manifest.time_range.ended_at < manifest.time_range.started_at {
        errors.push(issue(
            "invalid_bundle",
            "time_range.ended_at must be >= started_at",
        ));
    }
    if manifest.source.device_id.is_empty() || manifest.source.device_key_id.is_empty() {
        errors.push(issue(
            "invalid_bundle",
            "source.device_id and source.device_key_id are required",
        ));
    }
    if manifest.transcript_segments.len() > MAX_TRANSCRIPT_SEGMENTS {
        errors.push(issue(
            "bound_exceeded",
            format!("transcript_segments exceeds {MAX_TRANSCRIPT_SEGMENTS}"),
        ));
    }
    for segment in &manifest.transcript_segments {
        if segment.text.len() > MAX_SEGMENT_TEXT_BYTES {
            errors.push(issue(
                "bound_exceeded",
                format!("transcript segment {} exceeds text bound", segment.id),
            ));
        }
        if looks_iso8601(&segment.started_at)
            && looks_iso8601(&segment.ended_at)
            && segment.ended_at < segment.started_at
        {
            errors.push(issue(
                "invalid_bundle",
                format!("transcript segment {} timing is inverted", segment.id),
            ));
        }
    }
    if manifest.media.len() > MAX_MEDIA_REFS {
        errors.push(issue("bound_exceeded", "media exceeds bound"));
    }
    let mut media_ids = std::collections::HashSet::new();
    let mut media_hashes = std::collections::HashSet::new();
    for media in &manifest.media {
        if !media_ids.insert(media.id.clone()) {
            errors.push(issue(
                "duplicate_media_id",
                format!("duplicate media id {}", media.id),
            ));
        }
        if media.content_hash.algorithm != ALLOWED_HASH_ALG {
            errors.push(issue(
                "invalid_bundle",
                format!(
                    "media {} hash algorithm must be {ALLOWED_HASH_ALG}",
                    media.id
                ),
            ));
        }
        if !valid_sha(&media.content_hash.hex) {
            errors.push(issue(
                "invalid_bundle",
                format!("media {} has an invalid content hash", media.id),
            ));
        } else if !media_hashes.insert(media.content_hash.hex.clone()) {
            errors.push(issue(
                "duplicate_media_hash",
                format!("duplicate media hash for {}", media.id),
            ));
        }
        if media.byte_length > MAX_ARTIFACT_BYTES {
            errors.push(issue(
                "bound_exceeded",
                format!("media {} byte_length is out of bounds", media.id),
            ));
        }
        if !is_safe_relative_path(&media.relative_path) {
            errors.push(issue(
                "invalid_bundle",
                format!("media {} relative_path is unsafe", media.id),
            ));
        }
        if media.protection != PROTECTION_NONE && media.protection != PROTECTION_ENCRYPTED_HANDOFF {
            errors.push(issue(
                "invalid_bundle",
                format!(
                    "media {} has unknown protection {}",
                    media.id, media.protection
                ),
            ));
        }
    }
    if manifest.windows.len() > MAX_WINDOWS {
        errors.push(issue("bound_exceeded", "windows exceeds bound"));
    }
    if manifest.proposed_task.title.chars().count() > MAX_TITLE_CHARS {
        errors.push(issue("bound_exceeded", "proposed_task.title exceeds bound"));
    }
    if manifest.proposed_task.summary.len() > MAX_SUMMARY_BYTES {
        errors.push(issue(
            "bound_exceeded",
            "proposed_task.summary exceeds bound",
        ));
    }
    if manifest.proposed_task.acceptance_criteria.len() > MAX_ACCEPTANCE_CRITERIA {
        errors.push(issue("bound_exceeded", "acceptance_criteria exceeds bound"));
    }
    for c in &manifest.proposed_task.acceptance_criteria {
        if c.chars().count() > MAX_CRITERION_CHARS {
            errors.push(issue("bound_exceeded", "acceptance criterion too long"));
        }
    }
    if manifest.proposed_task.steps.len() > MAX_STEPS {
        errors.push(issue("bound_exceeded", "steps exceeds bound"));
    }
    for s in &manifest.proposed_task.steps {
        if s.chars().count() > MAX_STEP_CHARS {
            errors.push(issue("bound_exceeded", "step too long"));
        }
    }
    if manifest.privacy_labels.len() > MAX_PRIVACY_LABELS {
        errors.push(issue("bound_exceeded", "privacy_labels exceeds bound"));
    }
    for label in &manifest.privacy_labels {
        if !PRIVACY_LABELS.contains(&label.as_str()) {
            errors.push(issue(
                "invalid_bundle",
                format!("unknown privacy label {label}"),
            ));
        }
    }
    if manifest.redactions.len() > MAX_REDACTIONS {
        errors.push(issue("bound_exceeded", "redactions exceeds bound"));
    }
    errors
}

fn validate_redactions(manifest: &EvidenceManifest) -> Vec<EvidenceIssue> {
    let mut errors = Vec::new();
    for segment in &manifest.transcript_segments {
        if segment.redacted && !is_redacted_text(&segment.text) {
            errors.push(issue(
                "redacted_segment_leaked",
                format!(
                    "transcript segment {} is marked redacted but still contains text",
                    segment.id
                ),
            ));
        }
    }
    for redaction in &manifest.redactions {
        if redaction.target != "transcript" {
            continue;
        }
        match manifest
            .transcript_segments
            .iter()
            .find(|s| s.id == redaction.target_id)
        {
            None => errors.push(issue(
                "invalid_bundle",
                format!("redaction target {} is missing", redaction.target_id),
            )),
            Some(segment) if !segment.redacted || !is_redacted_text(&segment.text) => {
                errors.push(issue(
                    "redacted_segment_leaked",
                    format!(
                        "redaction {} did not clear transcript text",
                        redaction.target_id
                    ),
                ));
            }
            Some(_) => {}
        }
    }
    errors
}

fn media_bytes(
    media: &EvidenceMediaRef,
    ctx: &ValidationContext,
) -> Result<Option<Vec<u8>>, Vec<EvidenceIssue>> {
    if let Some(bytes) = ctx.artifacts.get(&media.artifact_id) {
        return Ok(Some(bytes.clone()));
    }
    let Some(root) = ctx.media_root.as_ref() else {
        return Err(vec![issue(
            "missing_media",
            format!("artifact {} is not present", media.artifact_id),
        )]);
    };
    let candidates = [
        media.file_name.as_deref(),
        Some(media.relative_path.as_str()),
    ];
    for candidate in candidates.into_iter().flatten() {
        match contained_join(root, candidate) {
            Ok(path) if path.is_file() => {
                let bytes = std::fs::read(&path).map_err(|e| {
                    vec![issue(
                        "missing_media",
                        format!("artifact {} read failed: {e}", media.artifact_id),
                    )]
                })?;
                return Ok(Some(bytes));
            }
            Ok(_) => continue,
            Err(e) => {
                return Err(vec![issue("invalid_bundle", e)]);
            }
        }
    }
    if media.protection == PROTECTION_ENCRYPTED_HANDOFF {
        // Source may still live in the recording folder under file_name; if not found,
        // treat as pending handoff rather than inventing encryption.
        return Ok(None);
    }
    Err(vec![issue(
        "missing_media",
        format!("artifact {} is not present", media.artifact_id),
    )])
}

fn validate_media(manifest: &EvidenceManifest, ctx: &ValidationContext) -> Vec<EvidenceIssue> {
    let mut errors = Vec::new();
    for media in &manifest.media {
        match media_bytes(media, ctx) {
            Err(inner) => errors.extend(inner),
            Ok(None) if media.protection == PROTECTION_ENCRYPTED_HANDOFF => {
                // Explicit handoff required; source retained elsewhere.
            }
            Ok(None) => errors.push(issue(
                "missing_media",
                format!("artifact {} is not present", media.artifact_id),
            )),
            Ok(Some(bytes)) => {
                if bytes.len() as u64 != media.byte_length {
                    errors.push(issue(
                        "content_hash_mismatch",
                        format!(
                            "artifact {} length {} != {}",
                            media.artifact_id,
                            bytes.len(),
                            media.byte_length
                        ),
                    ));
                }
                if sha256_hex(&bytes) != media.content_hash.hex {
                    errors.push(issue(
                        "content_hash_mismatch",
                        format!(
                            "artifact {} hash does not match the manifest",
                            media.artifact_id
                        ),
                    ));
                }
            }
        }
    }
    let _ = hash_file; // keep import available for callers
    errors
}

fn validate_signature(manifest: &EvidenceManifest, ctx: &ValidationContext) -> Vec<EvidenceIssue> {
    let mut errors = Vec::new();
    let Some(sig) = &manifest.signature else {
        errors.push(issue("unsigned_manifest", "manifest is not signed"));
        return errors;
    };
    if sig.algorithm != ALLOWED_SIG_ALG {
        errors.push(issue(
            "invalid_bundle",
            format!("signature algorithm must be {ALLOWED_SIG_ALG}"),
        ));
    }
    if !ctx.trusted_key_ids.iter().any(|id| id == &sig.key_id) {
        errors.push(issue(
            "untrusted_signer",
            format!("signer {} is not in the trust store", sig.key_id),
        ));
    }
    let Some(key_hex) = ctx.device_keys.get(&sig.key_id) else {
        if ctx.trusted_key_ids.iter().any(|id| id == &sig.key_id) {
            errors.push(issue(
                "untrusted_signer",
                format!("trusted key {} material is unavailable", sig.key_id),
            ));
        }
        return errors;
    };
    match (hex_decode(key_hex), signing_payload(manifest)) {
        (Ok(key), Ok(payload)) => {
            let expected = hmac_sha256_hex(&key, payload.as_bytes());
            if expected != sig.hex || sig.key_id != manifest.source.device_key_id {
                errors.push(issue(
                    "tampered_manifest",
                    "signature does not match the canonical manifest",
                ));
            }
        }
        _ => errors.push(issue(
            "tampered_manifest",
            "signature does not match the canonical manifest",
        )),
    }
    errors
}

pub fn validate_evidence_bundle(
    manifest: &EvidenceManifest,
    ctx: &ValidationContext,
) -> EvidenceValidationReport {
    let mut errors = Vec::new();
    errors.extend(validate_structure(manifest));
    errors.extend(validate_redactions(manifest));
    errors.extend(validate_media(manifest, ctx));
    errors.extend(validate_signature(manifest, ctx));
    if ctx.check_duplicate_import
        && ctx
            .imported_bundle_ids
            .iter()
            .any(|id| id == &manifest.bundle_id)
    {
        errors.push(issue(
            "duplicate_import",
            format!("bundle {} was already imported", manifest.bundle_id),
        ));
    }
    EvidenceValidationReport {
        ok: errors.is_empty(),
        errors,
        bundle_id: Some(manifest.bundle_id.clone()),
    }
}

pub fn assert_reviewed_before_task_create(manifest: &EvidenceManifest) -> Vec<EvidenceIssue> {
    let mut errors = Vec::new();
    if !manifest.review.privacy_reviewed || manifest.review.reviewed_at.is_none() {
        errors.push(issue(
            "review_required",
            "privacy review must be completed before creating a Task draft",
        ));
    }
    if manifest.signature.is_none() {
        errors.push(issue(
            "unsigned_manifest",
            "a Task draft requires a signed EvidenceBundle",
        ));
    }
    if manifest.proposed_task.title.trim().is_empty() {
        errors.push(issue("review_required", "proposed title must be reviewed"));
    }
    errors
}

pub fn create_task_draft(
    manifest: &EvidenceManifest,
    ctx: &ValidationContext,
    scope: &str,
    project_workspace: Option<String>,
    provider: Option<String>,
) -> (Option<EvidenceTaskDraft>, EvidenceValidationReport) {
    let mut report = validate_evidence_bundle(manifest, ctx);
    report
        .errors
        .extend(assert_reviewed_before_task_create(manifest));
    report.ok = report.errors.is_empty();
    if !report.ok {
        return (None, report);
    }
    (
        Some(EvidenceTaskDraft {
            source_bundle_id: manifest.bundle_id.clone(),
            title: manifest.proposed_task.title.clone(),
            summary: manifest.proposed_task.summary.clone(),
            acceptance_criteria: manifest.proposed_task.acceptance_criteria.clone(),
            steps: manifest.proposed_task.steps.clone(),
            selected_media_ids: manifest.media.iter().map(|m| m.id.clone()).collect(),
            selected_transcript_ids: manifest
                .transcript_segments
                .iter()
                .filter(|s| !s.redacted)
                .map(|s| s.id.clone())
                .collect(),
            scope: scope.into(),
            project_workspace,
            provider,
        }),
        report,
    )
}

#[cfg(test)]
mod evidence_bundle {
    use super::*;
    use crate::evidence::sign::sign_manifest;
    use crate::evidence::test_support::{
        alt_identity, fixture_identity, load_fixture, test_identity,
    };

    fn ctx_for(identity: &DeviceIdentity) -> ValidationContext {
        ValidationContext {
            trusted_key_ids: vec![identity.device_key_id.clone()],
            imported_bundle_ids: vec![],
            device_keys: [(
                identity.device_key_id.clone(),
                identity.hmac_key_hex.clone(),
            )]
            .into_iter()
            .collect(),
            artifacts: Default::default(),
            media_root: None,
            check_duplicate_import: true,
        }
    }

    fn signed(name: &str, identity: &DeviceIdentity) -> EvidenceManifest {
        sign_manifest(&load_fixture(name), identity, "2026-08-11T19:01:00.000Z").unwrap()
    }

    #[test]
    fn accepts_signed_minimal_bundle() {
        let identity = test_identity();
        let manifest = signed("minimal.json", &identity);
        let mut ctx = ctx_for(&identity);
        ctx.artifacts.insert("art-1".into(), vec![]);
        ctx.check_duplicate_import = false;
        assert!(validate_evidence_bundle(&manifest, &ctx).ok);
    }

    #[test]
    fn export_validation_skips_duplicate_import() {
        let identity = test_identity();
        let manifest = signed("minimal.json", &identity);
        let mut ctx = ctx_for(&identity);
        ctx.artifacts.insert("art-1".into(), vec![]);
        ctx.imported_bundle_ids.push(manifest.bundle_id.clone());
        ctx.check_duplicate_import = false;
        assert!(validate_evidence_bundle(&manifest, &ctx).ok);
        ctx.check_duplicate_import = true;
        assert!(validate_evidence_bundle(&manifest, &ctx).has_code("duplicate_import"));
    }

    #[test]
    fn rejects_path_escape() {
        assert!(!is_safe_relative_path("../etc/passwd"));
        assert!(!is_safe_relative_path("/abs"));
        assert!(is_safe_relative_path("recording.webm"));
    }

    #[test]
    fn rejects_unsupported_schema() {
        let identity = test_identity();
        let manifest = signed("bad-schema-version.json", &identity);
        assert!(validate_evidence_bundle(&manifest, &ctx_for(&identity))
            .has_code("unsupported_schema_version"));
    }

    #[test]
    fn rejects_invalid_timezone() {
        let identity = test_identity();
        let manifest = signed("bad-timezone.json", &identity);
        assert!(
            validate_evidence_bundle(&manifest, &ctx_for(&identity)).has_code("invalid_timezone")
        );
    }

    #[test]
    fn rejects_redacted_leak() {
        let identity = test_identity();
        let manifest = signed("redacted-leak.json", &identity);
        assert!(validate_evidence_bundle(&manifest, &ctx_for(&identity))
            .has_code("redacted_segment_leaked"));
    }

    #[test]
    fn rejects_missing_media() {
        let identity = test_identity();
        let manifest = signed("missing-media.json", &identity);
        assert!(validate_evidence_bundle(&manifest, &ctx_for(&identity)).has_code("missing_media"));
    }

    #[test]
    fn rejects_content_hash_mismatch() {
        let identity = test_identity();
        let manifest = signed("minimal.json", &identity);
        let mut ctx = ctx_for(&identity);
        ctx.artifacts
            .insert("art-1".into(), b"tampered-bytes".to_vec());
        assert!(validate_evidence_bundle(&manifest, &ctx).has_code("content_hash_mismatch"));
    }

    #[test]
    fn rejects_tampered_manifest() {
        let identity = test_identity();
        let mut manifest = signed("minimal.json", &identity);
        manifest.proposed_task.title = "tampered title".into();
        let mut ctx = ctx_for(&identity);
        ctx.artifacts.insert("art-1".into(), vec![]);
        assert!(validate_evidence_bundle(&manifest, &ctx).has_code("tampered_manifest"));
    }

    #[test]
    fn rejects_untrusted_signer() {
        let identity = test_identity();
        let manifest = signed("minimal.json", &identity);
        let mut ctx = ctx_for(&alt_identity());
        ctx.artifacts.insert("art-1".into(), vec![]);
        assert!(validate_evidence_bundle(&manifest, &ctx).has_code("untrusted_signer"));
    }

    #[test]
    fn review_required_before_task_draft() {
        let identity = test_identity();
        let mut manifest = load_fixture("minimal.json");
        manifest.review.privacy_reviewed = false;
        manifest.review.reviewed_at = None;
        let manifest = sign_manifest(&manifest, &identity, "2026-08-11T19:01:00.000Z").unwrap();
        let mut ctx = ctx_for(&identity);
        ctx.artifacts.insert("art-1".into(), vec![]);
        let (draft, report) = create_task_draft(&manifest, &ctx, "personal", None, None);
        assert!(draft.is_none());
        assert!(report.has_code("review_required"));
    }

    #[test]
    fn creates_personal_draft_after_review() {
        let identity = test_identity();
        let manifest = signed("minimal.json", &identity);
        let mut ctx = ctx_for(&identity);
        ctx.artifacts.insert("art-1".into(), vec![]);
        let (draft, report) = create_task_draft(&manifest, &ctx, "personal", None, None);
        assert!(report.ok);
        assert_eq!(draft.unwrap().scope, "personal");
    }

    #[test]
    fn accepts_full_fixture_with_separate_media() {
        let identity = fixture_identity("dev-fixture-2", "key-fixture-2", "33");
        let manifest = signed("full.json", &identity);
        let mut ctx = ctx_for(&identity);
        ctx.artifacts.insert("art-rec".into(), b"abc".to_vec());
        ctx.artifacts.insert("art-shot".into(), vec![]);
        let report = validate_evidence_bundle(&manifest, &ctx);
        assert!(report.ok, "{:?}", report.errors);
    }
}
