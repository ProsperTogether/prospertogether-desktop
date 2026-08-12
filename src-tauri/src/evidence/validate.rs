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

fn parse_digits(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    bytes.iter().try_fold(0u32, |value, digit| {
        value.checked_mul(10)?.checked_add(u32::from(*digit - b'0'))
    })
}

fn leap_year(year: u32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

// Gregorian calendar day number relative to 1970-01-01.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let adjusted_year = year - if month <= 2 { 1 } else { 0 };
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let month_prime = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Parse the canonical UTC timestamp form used by EvidenceBundle v1.
/// Returns milliseconds since Unix epoch only for real calendar values.
fn timestamp_millis(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if !(bytes.len() == 20 || (22..=24).contains(&bytes.len()))
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || *bytes.last()? != b'Z'
    {
        return None;
    }
    if bytes.len() != 20 && bytes[19] != b'.' {
        return None;
    }
    let year = parse_digits(&bytes[0..4])?;
    let month = parse_digits(&bytes[5..7])?;
    let day = parse_digits(&bytes[8..10])?;
    let hour = parse_digits(&bytes[11..13])?;
    let minute = parse_digits(&bytes[14..16])?;
    let second = parse_digits(&bytes[17..19])?;
    if !(1..=9999).contains(&year)
        || !(1..=12).contains(&month)
        || !(1..=days_in_month(year, month)).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let fraction_millis = if bytes.len() == 20 {
        0
    } else {
        let fraction = parse_digits(&bytes[20..bytes.len() - 1])?;
        match bytes.len() - 21 {
            1 => fraction * 100,
            2 => fraction * 10,
            3 => fraction,
            _ => return None,
        }
    };
    let seconds = days_from_civil(year as i64, month as i64, day as i64)
        .checked_mul(86_400)?
        .checked_add(i64::from(hour) * 3_600 + i64::from(minute) * 60 + i64::from(second))?;
    seconds
        .checked_mul(1_000)?
        .checked_add(i64::from(fraction_millis))
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

fn is_safe_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_BUNDLE_ID_CHARS
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

/// Reject absolute paths, `..`, and backslashes. Relative names only.
pub fn is_safe_relative_path(path: &str) -> bool {
    if path.is_empty()
        || path.len() > MAX_PATH_CHARS
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.contains('\\')
        || path.contains('\0')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == ".." || part.contains(':'))
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
    let started_at = timestamp_millis(&manifest.time_range.started_at);
    let ended_at = timestamp_millis(&manifest.time_range.ended_at);
    if started_at.is_none()
        || ended_at.is_none()
        || timestamp_millis(&manifest.exported_at).is_none()
    {
        errors.push(issue(
            "invalid_bundle",
            "timestamps must be ISO-8601 UTC ending in Z",
        ));
    } else if let (Some(started_at), Some(ended_at)) = (started_at, ended_at) {
        if ended_at < started_at {
            errors.push(issue(
                "invalid_bundle",
                "time_range.ended_at must be >= started_at",
            ));
        } else if ended_at - started_at > MAX_CAPTURE_DURATION_MS {
            errors.push(issue(
                "bound_exceeded",
                format!("time_range exceeds {MAX_CAPTURE_DURATION_MS}ms"),
            ));
        }
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
        if !is_safe_identifier(&segment.id) {
            errors.push(issue(
                "invalid_bundle",
                format!("transcript segment {} has an invalid id", segment.id),
            ));
        }
        if segment.text.len() > MAX_SEGMENT_TEXT_BYTES {
            errors.push(issue(
                "bound_exceeded",
                format!("transcript segment {} exceeds text bound", segment.id),
            ));
        }
        let segment_started = timestamp_millis(&segment.started_at);
        let segment_ended = timestamp_millis(&segment.ended_at);
        if segment_started.is_none() || segment_ended.is_none() {
            errors.push(issue(
                "invalid_bundle",
                format!("transcript segment {} has invalid timestamps", segment.id),
            ));
        } else if segment_ended < segment_started {
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
        if !is_safe_identifier(&media.id) || !is_safe_identifier(&media.artifact_id) {
            errors.push(issue(
                "invalid_bundle",
                format!("media {} has an invalid id", media.id),
            ));
        }
        if !matches!(media.kind.as_str(), "recording" | "screenshot" | "audio") {
            errors.push(issue(
                "invalid_bundle",
                format!("media {} has an unsupported kind", media.id),
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
        if let Some(file_name) = media.file_name.as_deref() {
            if !is_safe_relative_path(file_name) {
                errors.push(issue(
                    "invalid_bundle",
                    format!("media {} file_name is unsafe", media.id),
                ));
            }
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
    for redaction in &manifest.redactions {
        if !matches!(
            redaction.target.as_str(),
            "transcript" | "media" | "window" | "proposed_task"
        ) {
            errors.push(issue(
                "invalid_bundle",
                format!(
                    "redaction {} has an unsupported target",
                    redaction.target_id
                ),
            ));
        }
        if !is_safe_identifier(&redaction.target_id) {
            errors.push(issue(
                "invalid_bundle",
                "redaction target id is missing or too long",
            ));
        }
        if timestamp_millis(&redaction.redacted_at).is_none() {
            errors.push(issue(
                "invalid_bundle",
                format!("redaction {} has an invalid timestamp", redaction.target_id),
            ));
        }
    }
    if let Some(reviewed_at) = manifest.review.reviewed_at.as_deref() {
        if timestamp_millis(reviewed_at).is_none() {
            errors.push(issue(
                "invalid_bundle",
                "reviewed_at must be a valid ISO-8601 UTC timestamp",
            ));
        }
    }
    if let Some(signature) = manifest.signature.as_ref() {
        if signature.key_id.is_empty() || !valid_sha(&signature.hex) {
            errors.push(issue(
                "invalid_bundle",
                "signature key_id and hex are required",
            ));
        }
        if timestamp_millis(&signature.signed_at).is_none() {
            errors.push(issue(
                "invalid_bundle",
                "signed_at must be a valid ISO-8601 UTC timestamp",
            ));
        }
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
        if redaction.target == "media" {
            if !manifest
                .media
                .iter()
                .any(|media| media.id == redaction.target_id)
            {
                errors.push(issue(
                    "invalid_bundle",
                    format!("redaction target {} is missing", redaction.target_id),
                ));
            }
            continue;
        }
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

fn validate_media(manifest: &EvidenceManifest, ctx: &ValidationContext) -> Vec<EvidenceIssue> {
    let mut errors = Vec::new();
    for media in &manifest.media {
        if let Some(bytes) = ctx.artifacts.get(&media.artifact_id) {
            if bytes.len() as u64 > MAX_ARTIFACT_BYTES {
                errors.push(issue(
                    "bound_exceeded",
                    format!("artifact {} exceeds size bound", media.artifact_id),
                ));
                continue;
            }
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
            if sha256_hex(bytes) != media.content_hash.hex {
                errors.push(issue(
                    "content_hash_mismatch",
                    format!(
                        "artifact {} hash does not match the manifest",
                        media.artifact_id
                    ),
                ));
            }
            continue;
        }

        let Some(root) = ctx.media_root.as_ref() else {
            if media.protection != PROTECTION_ENCRYPTED_HANDOFF {
                errors.push(issue(
                    "missing_media",
                    format!("artifact {} is not present", media.artifact_id),
                ));
            }
            continue;
        };
        let mut found = false;
        let candidates = [
            media.file_name.as_deref(),
            Some(media.relative_path.as_str()),
        ];
        for candidate in candidates.into_iter().flatten() {
            let path = match contained_join(root, candidate) {
                Ok(path) => path,
                Err(e) => {
                    errors.push(issue("invalid_bundle", e));
                    found = true;
                    break;
                }
            };
            if !path.is_file() {
                continue;
            }
            found = true;
            let metadata = match std::fs::metadata(&path) {
                Ok(metadata) => metadata,
                Err(e) => {
                    errors.push(issue(
                        "missing_media",
                        format!("artifact {} stat failed: {e}", media.artifact_id),
                    ));
                    break;
                }
            };
            if metadata.len() > MAX_ARTIFACT_BYTES {
                errors.push(issue(
                    "bound_exceeded",
                    format!("artifact {} exceeds size bound", media.artifact_id),
                ));
                break;
            }
            match hash_file(&path) {
                Ok((byte_length, hash)) => {
                    if byte_length != media.byte_length {
                        errors.push(issue(
                            "content_hash_mismatch",
                            format!(
                                "artifact {} length {} != {}",
                                media.artifact_id, byte_length, media.byte_length
                            ),
                        ));
                    }
                    if hash != media.content_hash.hex {
                        errors.push(issue(
                            "content_hash_mismatch",
                            format!(
                                "artifact {} hash does not match the manifest",
                                media.artifact_id
                            ),
                        ));
                    }
                }
                Err(e) => errors.push(issue(
                    "missing_media",
                    format!("artifact {} read failed: {e}", media.artifact_id),
                )),
            }
            break;
        }
        if !found && media.protection != PROTECTION_ENCRYPTED_HANDOFF {
            errors.push(issue(
                "missing_media",
                format!("artifact {} is not present", media.artifact_id),
            ));
        }
    }
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
    create_task_draft_with_selection(
        manifest,
        ctx,
        scope,
        project_workspace,
        provider,
        None,
        None,
    )
}

fn selection_issue(code: &str, message: impl Into<String>) -> EvidenceIssue {
    issue(code, message)
}

/// Resolve caller-selected evidence against the signed manifest. Explicit
/// selections are never allowed to smuggle in missing or redacted content.
pub fn resolve_task_selection(
    manifest: &EvidenceManifest,
    selected_media_ids: Option<&[String]>,
    selected_transcript_ids: Option<&[String]>,
) -> Result<(Vec<String>, Vec<String>), Vec<EvidenceIssue>> {
    let mut errors = Vec::new();
    let default_media: Vec<String> = manifest
        .media
        .iter()
        .map(|media| media.id.clone())
        .collect();
    let default_transcript: Vec<String> = manifest
        .transcript_segments
        .iter()
        .filter(|segment| !segment.redacted)
        .map(|segment| segment.id.clone())
        .collect();
    let media_ids = selected_media_ids.unwrap_or(&default_media);
    let transcript_ids = selected_transcript_ids.unwrap_or(&default_transcript);

    let mut seen_media = std::collections::HashSet::new();
    let redacted_media: std::collections::HashSet<&str> = manifest
        .redactions
        .iter()
        .filter(|redaction| redaction.target == "media")
        .map(|redaction| redaction.target_id.as_str())
        .collect();
    for id in media_ids {
        if !seen_media.insert(id.as_str()) {
            errors.push(selection_issue(
                "invalid_bundle",
                format!("media {id} was selected more than once"),
            ));
            continue;
        }
        let Some(media) = manifest.media.iter().find(|media| media.id == *id) else {
            errors.push(selection_issue(
                "invalid_bundle",
                format!("selected media {id} is not in the manifest"),
            ));
            continue;
        };
        if redacted_media.contains(id.as_str()) {
            errors.push(selection_issue(
                "invalid_bundle",
                format!("selected media {id} is redacted"),
            ));
        }
        if media.protection == PROTECTION_ENCRYPTED_HANDOFF {
            errors.push(selection_issue(
                "encrypted_object_handoff_required",
                format!("selected media {id} requires an unavailable encrypted-object handoff"),
            ));
        }
    }

    let mut seen_transcript = std::collections::HashSet::new();
    for id in transcript_ids {
        if !seen_transcript.insert(id.as_str()) {
            errors.push(selection_issue(
                "invalid_bundle",
                format!("transcript {id} was selected more than once"),
            ));
            continue;
        }
        let Some(segment) = manifest
            .transcript_segments
            .iter()
            .find(|segment| segment.id == *id)
        else {
            errors.push(selection_issue(
                "invalid_bundle",
                format!("selected transcript {id} is not in the manifest"),
            ));
            continue;
        };
        if segment.redacted {
            errors.push(selection_issue(
                "invalid_bundle",
                format!("selected transcript {id} is redacted"),
            ));
        }
    }
    if errors.is_empty() {
        Ok((media_ids.to_vec(), transcript_ids.to_vec()))
    } else {
        Err(errors)
    }
}

pub fn create_task_draft_with_selection(
    manifest: &EvidenceManifest,
    ctx: &ValidationContext,
    scope: &str,
    project_workspace: Option<String>,
    provider: Option<String>,
    selected_media_ids: Option<&[String]>,
    selected_transcript_ids: Option<&[String]>,
) -> (Option<EvidenceTaskDraft>, EvidenceValidationReport) {
    let mut report = validate_evidence_bundle(manifest, ctx);
    report
        .errors
        .extend(assert_reviewed_before_task_create(manifest));
    let selection = resolve_task_selection(manifest, selected_media_ids, selected_transcript_ids);
    if let Err(selection_errors) = &selection {
        report.errors.extend(selection_errors.clone());
    }
    report.ok = report.errors.is_empty();
    if !report.ok {
        return (None, report);
    }
    let (selected_media_ids, selected_transcript_ids) = selection.expect("selection checked above");
    (
        Some(EvidenceTaskDraft {
            source_bundle_id: manifest.bundle_id.clone(),
            title: manifest.proposed_task.title.clone(),
            summary: manifest.proposed_task.summary.clone(),
            acceptance_criteria: manifest.proposed_task.acceptance_criteria.clone(),
            steps: manifest.proposed_task.steps.clone(),
            selected_media_ids,
            selected_transcript_ids,
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
