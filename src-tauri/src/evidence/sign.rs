use super::canonical::{canonical_json, unsigned_value};
use super::limits::ALLOWED_SIG_ALG;
use super::types::{DeviceIdentity, EvidenceManifest, EvidenceSignature, SignerProvider};

pub fn signing_payload(bundle: &EvidenceManifest) -> Result<String, String> {
    let mut unsigned = bundle.clone();
    unsigned.signature = None;
    let value = serde_json::to_value(&unsigned).map_err(|e| e.to_string())?;
    canonical_json(&unsigned_value(&value)?)
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
