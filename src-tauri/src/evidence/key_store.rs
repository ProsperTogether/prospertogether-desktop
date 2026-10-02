//! Durable evidence signing identity storage.
//!
//! The evidence HMAC key is an enrollment identity, not an ephemeral session
//! secret.  It must survive an application restart so Portal can continue to
//! trust bundles from the same device.  This module deliberately does not
//! write the key (or a JSON representation of it) to the app-data directory:
//! on Windows the record is protected with the current user's DPAPI key.  A
//! platform without an OS-backed implementation fails closed instead of
//! falling back to plaintext storage.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::crypto::{hex_decode, hex_encode, sha256_hex};
use super::types::{DeviceIdentity, DevicePublicIdentity};

const SECRET_FILENAME: &str = "device_secret.dpapi";
const RECORD_VERSION: u32 = 1;
const MAX_PROTECTED_BYTES: usize = 16 * 1024;
const MAX_ID_CHARS: usize = 128;
const KEY_BYTES: usize = 32;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SecretRecord {
    version: u32,
    device_id: String,
    device_key_id: String,
    hmac_key_hex: String,
}

impl std::fmt::Debug for SecretRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretRecord")
            .field("version", &self.version)
            .field("device_id", &self.device_id)
            .field("device_key_id", &self.device_key_id)
            .field("hmac_key_hex", &"[redacted]")
            .finish()
    }
}

/// Load the enrolled identity, or create it once when this device has never
/// enrolled.  `public_identity` is the non-secret compatibility file already
/// used by the app; the protected record is authoritative for the key and its
/// identity.  A mismatch is an error rather than an automatic rotation,
/// because rotating silently would orphan the device's Portal enrollment.
pub fn load_or_create(
    root: &Path,
    public_identity: Option<&DevicePublicIdentity>,
) -> Result<DeviceIdentity, String> {
    if let Some(identity) = load_protected(root)? {
        if let Some(public) = public_identity {
            if public.device_id != identity.device_id
                || public.device_key_id != identity.device_key_id
            {
                return Err("public evidence identity does not match protected enrollment".into());
            }
        }
        return Ok(identity);
    }

    if public_identity.is_some() {
        return Err(
            "protected evidence identity is missing; refuse to mint a replacement key".into(),
        );
    }

    let public = generate_public_identity();
    let key = generate_key_hex();
    let record = SecretRecord {
        version: RECORD_VERSION,
        device_id: public.device_id.clone(),
        device_key_id: public.device_key_id.clone(),
        hmac_key_hex: key.clone(),
    };
    let plaintext = serde_json::to_vec(&record).map_err(|e| e.to_string())?;
    let encrypted = protect(&plaintext)?;
    if encrypted.len() > MAX_PROTECTED_BYTES {
        return Err("protected evidence identity is too large".into());
    }
    write_new_atomic(&root.join(SECRET_FILENAME), &encrypted)?;
    Ok(DeviceIdentity {
        device_id: public.device_id,
        device_key_id: public.device_key_id,
        hmac_key_hex: key,
    })
}

/// Install the one-time secret returned by Portal's host-enrollment flow.
/// This is intentionally an explicit operation: an arbitrary caller cannot
/// rotate a valid identity merely by starting the app, and the public identity
/// remains bound to the protected record.
pub fn install_enrollment_secret(
    root: &Path,
    public_identity: &DevicePublicIdentity,
    enrollment_secret_hex: &str,
) -> Result<DeviceIdentity, String> {
    validate_public_identity(public_identity)?;
    let existing = load_protected(root)?.ok_or_else(|| {
        "protected evidence identity is missing; refuse to enroll a replacement".to_string()
    })?;
    if existing.device_id != public_identity.device_id
        || existing.device_key_id != public_identity.device_key_id
    {
        return Err("Portal enrollment identity does not match this device".into());
    }
    let key = hex_decode(enrollment_secret_hex)
        .map_err(|_| "Portal enrollment secret is not valid hex".to_string())?;
    if key.len() != KEY_BYTES {
        return Err("Portal enrollment secret has an invalid length".into());
    }
    let identity = DeviceIdentity {
        device_id: public_identity.device_id.clone(),
        device_key_id: public_identity.device_key_id.clone(),
        hmac_key_hex: hex_encode(&key),
    };
    let record = SecretRecord {
        version: RECORD_VERSION,
        device_id: identity.device_id.clone(),
        device_key_id: identity.device_key_id.clone(),
        hmac_key_hex: identity.hmac_key_hex.clone(),
    };
    let plaintext = serde_json::to_vec(&record).map_err(|e| e.to_string())?;
    let encrypted = protect(&plaintext)?;
    if encrypted.len() > MAX_PROTECTED_BYTES {
        return Err("protected evidence identity is too large".into());
    }
    replace_atomic(&root.join(SECRET_FILENAME), &encrypted)?;
    Ok(identity)
}

fn load_protected(root: &Path) -> Result<Option<DeviceIdentity>, String> {
    let path = root.join(SECRET_FILENAME);
    match std::fs::read(&path) {
        Ok(bytes) => {
            if bytes.len() > MAX_PROTECTED_BYTES {
                return Err("stored evidence identity is too large".into());
            }
            let plaintext = unprotect(&bytes)?;
            let record: SecretRecord = serde_json::from_slice(&plaintext)
                .map_err(|_| "stored evidence identity is corrupt".to_string())?;
            Ok(Some(record.into_identity()?))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("read protected evidence identity: {error}")),
    }
}

impl SecretRecord {
    fn into_identity(self) -> Result<DeviceIdentity, String> {
        if self.version != RECORD_VERSION {
            return Err("stored evidence identity has an unsupported version".into());
        }
        let public = DevicePublicIdentity {
            device_id: self.device_id,
            device_key_id: self.device_key_id,
        };
        validate_public_identity(&public)?;
        let key = hex_decode(&self.hmac_key_hex)
            .map_err(|_| "stored evidence identity has an invalid signing key".to_string())?;
        if key.len() != KEY_BYTES {
            return Err("stored evidence signing key has an invalid length".into());
        }
        Ok(DeviceIdentity {
            device_id: public.device_id,
            device_key_id: public.device_key_id,
            hmac_key_hex: hex_encode(&key),
        })
    }
}

fn validate_public_identity(identity: &DevicePublicIdentity) -> Result<(), String> {
    for (label, value) in [
        ("device_id", identity.device_id.as_str()),
        ("device_key_id", identity.device_key_id.as_str()),
    ] {
        if value.is_empty() || value.chars().count() > MAX_ID_CHARS {
            return Err(format!("stored evidence {label} is invalid"));
        }
        if !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return Err(format!(
                "stored evidence {label} contains unsafe characters"
            ));
        }
    }
    Ok(())
}

fn generate_public_identity() -> DevicePublicIdentity {
    let a = uuid::Uuid::new_v4();
    let b = uuid::Uuid::new_v4();
    let mut raw = [0u8; 32];
    raw[..16].copy_from_slice(a.as_bytes());
    raw[16..].copy_from_slice(b.as_bytes());
    let digest = sha256_hex(&raw);
    DevicePublicIdentity {
        device_id: format!("devagent-{}", &digest[..12]),
        device_key_id: format!("key-{}", &digest[12..28]),
    }
}

fn generate_key_hex() -> String {
    let a = uuid::Uuid::new_v4();
    let b = uuid::Uuid::new_v4();
    let mut raw = [0u8; KEY_BYTES];
    raw[..16].copy_from_slice(a.as_bytes());
    raw[16..].copy_from_slice(b.as_bytes());
    hex_encode(&raw)
}

fn write_new_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::fs::OpenOptions;
    use std::io::Write;

    let parent = path
        .parent()
        .ok_or_else(|| "protected evidence identity has no parent".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("create evidence identity directory: {e}"))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                "protected evidence identity already exists".into()
            } else {
                format!("create protected evidence identity: {error}")
            }
        })?;
    let result = (|| {
        file.write_all(bytes)
            .map_err(|e| format!("write protected evidence identity: {e}"))?;
        file.sync_all()
            .map_err(|e| format!("sync protected evidence identity: {e}"))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result
}

#[cfg(target_os = "windows")]
fn replace_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::ffi::OsStr;
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let parent = path
        .parent()
        .ok_or_else(|| "protected evidence identity has no parent".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("create evidence identity directory: {e}"))?;
    let temporary = parent.join(format!(".{}.{}.tmp", SECRET_FILENAME, uuid::Uuid::new_v4()));
    let result = (|| {
        {
            use std::io::Write;
            let mut file = std::fs::File::create(&temporary)
                .map_err(|e| format!("create replacement evidence identity: {e}"))?;
            file.write_all(bytes)
                .map_err(|e| format!("write replacement evidence identity: {e}"))?;
            file.sync_all()
                .map_err(|e| format!("sync replacement evidence identity: {e}"))?;
        }
        let from: Vec<u16> = OsStr::new(&temporary)
            .encode_wide()
            .chain(once(0))
            .collect();
        let to: Vec<u16> = OsStr::new(path).encode_wide().chain(once(0)).collect();
        unsafe {
            MoveFileExW(
                PCWSTR(from.as_ptr()),
                PCWSTR(to.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
            .map_err(|e| format!("install replacement evidence identity: {e}"))?;
        }
        Ok::<(), String>(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(not(target_os = "windows"))]
fn replace_atomic(_path: &Path, _bytes: &[u8]) -> Result<(), String> {
    Err("secure evidence identity storage is unavailable on this platform".into())
}

#[cfg(target_os = "windows")]
fn protect(bytes: &[u8]) -> Result<Vec<u8>, String> {
    use std::ffi::c_void;
    use std::slice;

    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let cb_data = u32::try_from(bytes.len()).map_err(|_| "evidence identity is too large")?;
    let input = CRYPT_INTEGER_BLOB {
        cbData: cb_data,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(
            &input,
            windows::core::PCWSTR::null(),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|e| format!("protect evidence identity with Windows DPAPI: {e}"))?;
        if output.pbData.is_null() || output.cbData == 0 {
            return Err("Windows DPAPI returned an empty evidence identity".into());
        }
        let protected = slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(output.pbData as *mut c_void));
        Ok(protected)
    }
}

#[cfg(target_os = "windows")]
fn unprotect(bytes: &[u8]) -> Result<Vec<u8>, String> {
    use std::ffi::c_void;
    use std::slice;

    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let cb_data =
        u32::try_from(bytes.len()).map_err(|_| "stored evidence identity is too large")?;
    let input = CRYPT_INTEGER_BLOB {
        cbData: cb_data,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(
            &input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|e| format!("unprotect evidence identity with Windows DPAPI: {e}"))?;
        if output.pbData.is_null() || output.cbData == 0 {
            return Err("Windows DPAPI returned an empty evidence identity".into());
        }
        let plaintext = slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(output.pbData as *mut c_void));
        Ok(plaintext)
    }
}

#[cfg(not(target_os = "windows"))]
fn protect(_bytes: &[u8]) -> Result<Vec<u8>, String> {
    Err("secure evidence identity storage is unavailable on this platform".into())
}

#[cfg(not(target_os = "windows"))]
fn unprotect(_bytes: &[u8]) -> Result<Vec<u8>, String> {
    Err("secure evidence identity storage is unavailable on this platform".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_stable_and_key_is_not_written_in_plaintext() {
        let root =
            std::env::temp_dir().join(format!("devagent-evidence-key-{}", uuid::Uuid::new_v4()));
        let first = load_or_create(&root, None).expect("first identity should be created");
        let second = load_or_create(&root, None).expect("identity should be loaded");
        assert_eq!(first.device_id, second.device_id);
        assert_eq!(first.device_key_id, second.device_key_id);
        assert_eq!(first.hmac_key_hex, second.hmac_key_hex);

        let raw = std::fs::read(root.join(SECRET_FILENAME)).expect("protected identity exists");
        assert!(!raw
            .windows(first.hmac_key_hex.len())
            .any(|window| window == first.hmac_key_hex.as_bytes()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn public_identity_mismatch_fails_closed() {
        let root =
            std::env::temp_dir().join(format!("devagent-evidence-key-{}", uuid::Uuid::new_v4()));
        let first = load_or_create(&root, None).expect("first identity should be created");
        let wrong = DevicePublicIdentity {
            device_id: first.device_id,
            device_key_id: "key-other-device".into(),
        };
        let error = load_or_create(&root, Some(&wrong)).expect_err("mismatch must fail closed");
        assert!(error.contains("does not match"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn refuses_to_mint_a_replacement_key_when_public_identity_exists_without_protected_record() {
        let root =
            std::env::temp_dir().join(format!("devagent-evidence-key-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let public = DevicePublicIdentity {
            device_id: "devagent-existing".into(),
            device_key_id: "key-existing".into(),
        };
        let error = load_or_create(&root, Some(&public))
            .expect_err("missing protected record must not mint a replacement key");
        assert!(error.contains("missing") || error.contains("refuse"));
        assert!(!root.join(SECRET_FILENAME).exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn enrollment_refuses_a_mismatched_or_replacing_identity() {
        let root =
            std::env::temp_dir().join(format!("devagent-evidence-key-{}", uuid::Uuid::new_v4()));
        let first = load_or_create(&root, None).expect("first identity should be created");
        let mismatched = DevicePublicIdentity {
            device_id: "devagent-other".into(),
            device_key_id: first.device_key_id.clone(),
        };
        let error = install_enrollment_secret(&root, &mismatched, &"cd".repeat(KEY_BYTES))
            .expect_err("mismatched enrollment must fail closed");
        assert!(error.contains("match") || error.contains("identity"));

        let reloaded = load_or_create(&root, None).expect("original identity must remain");
        assert_eq!(reloaded.device_id, first.device_id);
        assert_eq!(reloaded.device_key_id, first.device_key_id);
        assert_eq!(reloaded.hmac_key_hex, first.hmac_key_hex);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn explicit_portal_secret_replaces_local_key_and_survives_reload() {
        let root =
            std::env::temp_dir().join(format!("devagent-evidence-key-{}", uuid::Uuid::new_v4()));
        let first = load_or_create(&root, None).expect("first identity should be created");
        let public = DevicePublicIdentity {
            device_id: first.device_id.clone(),
            device_key_id: first.device_key_id.clone(),
        };
        let enrolled_key = "ab".repeat(KEY_BYTES);
        let enrolled = install_enrollment_secret(&root, &public, &enrolled_key)
            .expect("Portal secret should be installed");
        assert_eq!(enrolled.hmac_key_hex, enrolled_key);
        let reloaded = load_or_create(&root, Some(&public)).expect("enrolled key should reload");
        assert_eq!(reloaded.hmac_key_hex, enrolled_key);
        let _ = std::fs::remove_dir_all(root);
    }
}
