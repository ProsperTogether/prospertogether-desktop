use super::types::{DeviceIdentity, EvidenceManifest};

pub fn test_identity() -> DeviceIdentity {
    fixture_identity("dev-fixture-1", "key-fixture-1", "11")
}

pub fn fixture_identity(device_id: &str, key_id: &str, hex_byte: &str) -> DeviceIdentity {
    DeviceIdentity {
        device_id: device_id.into(),
        device_key_id: key_id.into(),
        hmac_key_hex: hex_byte.repeat(32),
    }
}

pub fn alt_identity() -> DeviceIdentity {
    fixture_identity("dev-other", "key-other", "22")
}

pub fn load_fixture(name: &str) -> EvidenceManifest {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/fixtures/evidence/v1")
        .join(name);
    let raw =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {name}: {e}"))
}
