use std::collections::HashMap;
use std::path::Path;

use prospertogether_desktop_lib::evidence::{
    register_imported_bundle_id, resolve_task_selection, sign_manifest, validate_evidence_bundle,
    DeviceIdentity, EvidenceManifest, ValidationContext,
};

fn fixture() -> EvidenceManifest {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/evidence/v1/minimal.json");
    let raw = std::fs::read_to_string(path).expect("fixture should exist");
    serde_json::from_str(&raw).expect("fixture should parse")
}

fn identity() -> DeviceIdentity {
    DeviceIdentity {
        device_id: "dev-fixture-1".into(),
        device_key_id: "key-fixture-1".into(),
        hmac_key_hex: "11".repeat(32),
    }
}

fn context(identity: &DeviceIdentity) -> ValidationContext {
    ValidationContext {
        trusted_key_ids: vec![identity.device_key_id.clone()],
        imported_bundle_ids: vec![],
        device_keys: HashMap::from([(
            identity.device_key_id.clone(),
            identity.hmac_key_hex.clone(),
        )]),
        artifacts: HashMap::from([("art-1".into(), Vec::new())]),
        media_root: None,
        check_duplicate_import: false,
    }
}

#[test]
fn canonical_bundle_round_trip_rejects_bad_time() {
    let signer = identity();
    let signed = sign_manifest(&fixture(), &signer, "2026-08-11T19:01:00.000Z").unwrap();
    let json = serde_json::to_string(&signed).unwrap();
    assert!(json.contains("schemaVersion"));
    assert!(validate_evidence_bundle(&signed, &context(&signer)).ok);

    let mut bad = signed;
    bad.time_range.started_at = "2026-02-30T18:55:00.000Z".into();
    assert!(validate_evidence_bundle(&bad, &context(&signer)).has_code("invalid_bundle"));
}

#[test]
fn import_claim_is_separate_from_export_validation_and_selection_is_bound() {
    let root = std::env::temp_dir().join(format!("evidence-integration-{}", uuid::Uuid::new_v4()));
    assert!(register_imported_bundle_id(&root, "11111111-1111-4111-8111-111111111111").unwrap());
    assert!(!register_imported_bundle_id(&root, "11111111-1111-4111-8111-111111111111").unwrap());
    let _ = std::fs::remove_dir_all(root);

    let mut manifest = fixture();
    manifest.transcript_segments.push(
        prospertogether_desktop_lib::evidence::types::EvidenceTranscriptSegment {
            id: "seg-redacted".into(),
            started_at: "2026-08-11T18:55:00.000Z".into(),
            ended_at: "2026-08-11T18:56:00.000Z".into(),
            text: "[REDACTED]".into(),
            speaker: None,
            redacted: true,
        },
    );
    let errors = resolve_task_selection(
        &manifest,
        Some(&vec!["missing-media".into()]),
        Some(&vec!["seg-redacted".into()]),
    )
    .unwrap_err();
    assert!(errors.iter().any(|error| error.code == "invalid_bundle"));
}
