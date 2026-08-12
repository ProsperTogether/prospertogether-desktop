use super::canonical::canonical_json;
use super::limits::ALLOWED_SIG_ALG;
use super::types::{DeviceIdentity, EvidenceManifest, EvidenceSignature, SignerProvider};

pub fn signing_payload(bundle: &EvidenceManifest) -> Result<String, String> {
    let mut unsigned = bundle.clone();
    unsigned.signature = None;
    let value = serde_json::to_value(&unsigned).map_err(|e| e.to_string())?;
    // Keep an explicit null signature in the signed payload. This is the
    // canonical TS/Portal representation and makes cross-runtime signatures
    // interoperable instead of silently hashing two different documents.
    canonical_json(&value)
}

pub fn sign_manifest_with_provider(
    bundle: &EvidenceManifest,
    signer: &dyn SignerProvider,
    signed_at: &str,
) -> Result<EvidenceManifest, String> {
    let identity = signer.identity();
    let mut unsigned = bundle.clone();
    unsigned.source.device_id = identity.device_id.clone();
    unsigned.source.device_key_id = identity.device_key_id.clone();
    unsigned.signature = None;
    let payload = signing_payload(&unsigned)?;
    let hex = signer.sign_hex(payload.as_bytes())?;
    unsigned.signature = Some(EvidenceSignature {
        algorithm: ALLOWED_SIG_ALG.into(),
        key_id: identity.device_key_id,
        hex,
        signed_at: signed_at.into(),
    });
    Ok(unsigned)
}

/// Test helper that signs with an in-memory DeviceIdentity.
pub fn sign_manifest(
    bundle: &EvidenceManifest,
    identity: &DeviceIdentity,
    signed_at: &str,
) -> Result<EvidenceManifest, String> {
    let signer = super::types::InjectedHmacSigner {
        identity: identity.clone(),
    };
    sign_manifest_with_provider(bundle, &signer, signed_at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::test_support::{load_fixture, test_identity};

    #[test]
    fn signing_payload_keeps_explicit_null_signature() {
        let identity = test_identity();
        let manifest = sign_manifest(
            &load_fixture("minimal.json"),
            &identity,
            "2026-08-11T19:01:00.000Z",
        )
        .unwrap();
        let payload = signing_payload(&manifest).unwrap();
        assert!(payload.contains("\"signature\":null"));
        assert_eq!(
            payload.matches("\"signature\":null").count(),
            1,
            "canonical payload must contain exactly one explicit null signature"
        );
    }
}
