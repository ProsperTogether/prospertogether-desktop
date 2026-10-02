//! Atomic import-index persistence. Fail-closed on corruption.
//! Export must never write this index.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceImportIndex {
    pub bundle_ids: Vec<String>,
}

fn index_path(root: &Path) -> PathBuf {
    root.join("import_index.json")
}

fn lock_path(root: &Path) -> PathBuf {
    root.join("import_index.lock")
}

fn with_index_lock<T>(
    root: &Path,
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    std::fs::create_dir_all(root).map_err(|e| format!("create evidence root: {e}"))?;
    let mut lock_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(lock_path(root))
        .map_err(|_| "import index is locked or busy (fail-closed)".to_string())?;
    if let Err(error) = lock_file.write_all(b"1") {
        drop(lock_file);
        let _ = std::fs::remove_file(lock_path(root));
        return Err(format!("write lock: {error}"));
    }

    let result = operation();
    drop(lock_file);
    let _ = std::fs::remove_file(lock_path(root));
    result
}

fn write_index_locked(root: &Path, index: &EvidenceImportIndex) -> Result<(), String> {
    let tmp = root.join("import_index.json.tmp");
    let json = serde_json::to_string_pretty(index).map_err(|e| e.to_string())?;
    {
        let mut f = File::create(&tmp).map_err(|e| format!("create temp index: {e}"))?;
        f.write_all(json.as_bytes())
            .map_err(|e| format!("write temp index: {e}"))?;
        f.sync_all().map_err(|e| format!("sync temp index: {e}"))?;
    }
    std::fs::rename(&tmp, index_path(root)).map_err(|e| format!("rename index: {e}"))?;
    Ok(())
}

pub fn load_import_index(root: &Path) -> Result<EvidenceImportIndex, String> {
    let path = index_path(root);
    if !path.exists() {
        return Ok(EvidenceImportIndex::default());
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("read import index: {e}"))?;
    serde_json::from_str(&raw).map_err(|e| format!("import index corrupt (fail-closed): {e}"))
}

/// Exclusive lock + write temp + rename. Fail-closed if lock cannot be taken
/// or the existing index is corrupt.
pub fn save_import_index_atomic(root: &Path, index: &EvidenceImportIndex) -> Result<(), String> {
    with_index_lock(root, || {
        // Fail-closed: refuse to overwrite if current file exists but is corrupt.
        let _ = load_import_index(root)?;
        write_index_locked(root, index)
    })
}

pub fn register_imported_bundle_id(root: &Path, bundle_id: &str) -> Result<bool, String> {
    with_index_lock(root, || {
        let mut index = load_import_index(root)?;
        if index.bundle_ids.iter().any(|id| id == bundle_id) {
            return Ok(false);
        }
        index.bundle_ids.push(bundle_id.to_string());
        write_index_locked(root, &index)?;
        Ok(true)
    })
}

#[cfg(test)]
mod evidence_bundle {
    use super::*;
    use std::io::Read;

    #[test]
    fn atomic_write_and_corrupt_fail_closed() {
        let dir = std::env::temp_dir().join(format!("ev-idx-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut idx = EvidenceImportIndex::default();
        idx.bundle_ids.push("a".into());
        save_import_index_atomic(&dir, &idx).unwrap();
        assert_eq!(load_import_index(&dir).unwrap().bundle_ids, vec!["a"]);
        std::fs::write(index_path(&dir), "{not-json").unwrap();
        assert!(load_import_index(&dir).unwrap_err().contains("fail-closed"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn lock_file_is_released() {
        let dir = std::env::temp_dir().join(format!("ev-idx2-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        save_import_index_atomic(&dir, &EvidenceImportIndex::default()).unwrap();
        assert!(!lock_path(&dir).exists());
        let mut f = File::open(index_path(&dir)).unwrap();
        let mut s = String::new();
        f.read_to_string(&mut s).unwrap();
        assert!(s.contains("bundleIds") || s.contains("[]"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn imported_bundle_claim_is_idempotent_under_one_lock() {
        let dir = std::env::temp_dir().join(format!("ev-idx3-{}", uuid::Uuid::new_v4()));
        assert!(register_imported_bundle_id(&dir, "bundle-1").unwrap());
        assert!(register_imported_bundle_id(&dir, "bundle-2").unwrap());
        assert!(!register_imported_bundle_id(&dir, "bundle-1").unwrap());
        assert_eq!(
            load_import_index(&dir).unwrap().bundle_ids,
            vec!["bundle-1", "bundle-2"]
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
