//! Bundle identifier migration.
//!
//! When the app's bundle identifier changed from `com.userfirst.agent` to
//! `com.prospertogether.desktop`, Tauri's `app_data_dir()` started pointing
//! at a new folder. Users' stored settings (auth token, preferred audio
//! device, onboarding state) lived at the OLD path and would otherwise be
//! silently orphaned, forcing re-login and re-onboarding.
//!
//! This module runs once at startup (idempotent) and copies over:
//! - `%APPDATA%/com.userfirst.agent/settings.json` → new app data dir
//! - Any existing `recordings/` subfolder (forward-compat if a future build
//!   had durable recordings under the old identifier)
//! - `%TEMP%/userfirst/recording-state.json` → `%TEMP%/prospertogether/`
//!   (so `recover_orphaned_recording` still finds an in-flight ffmpeg)
//!
//! The migration is gated: if the new app data dir already exists with a
//! `settings.json`, we assume the migration already happened and skip.

use std::path::Path;

use tauri::{AppHandle, Manager};

const OLD_BUNDLE_ID: &str = "com.userfirst.agent";
const OLD_TEMP_SEGMENT: &str = "userfirst";
const NEW_TEMP_SEGMENT: &str = "prospertogether";

/// Run the bundle-identifier migration. Safe to call on every startup.
pub fn run_migration(app: &AppHandle) {
    if let Err(e) = migrate_app_data(app) {
        eprintln!("[migration] app data migration failed: {}", e);
    }
    if let Err(e) = migrate_temp_state() {
        eprintln!("[migration] temp state migration failed: {}", e);
    }
}

fn migrate_app_data(app: &AppHandle) -> Result<(), String> {
    let new_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir failed: {}", e))?;

    // If new dir already has settings.json, migration already happened.
    if new_dir.join("settings.json").exists() {
        return Ok(());
    }

    // Derive the old path from the new path's parent.
    let parent = match new_dir.parent() {
        Some(p) => p,
        None => return Ok(()),
    };
    let old_dir = parent.join(OLD_BUNDLE_ID);

    if !old_dir.exists() {
        // Nothing to migrate — fresh install.
        return Ok(());
    }

    eprintln!(
        "[migration] migrating app data from {} to {}",
        old_dir.display(),
        new_dir.display()
    );

    // Make sure the new dir exists.
    std::fs::create_dir_all(&new_dir)
        .map_err(|e| format!("create new app data dir: {}", e))?;

    // Copy settings.json if present.
    let old_settings = old_dir.join("settings.json");
    if old_settings.exists() {
        let new_settings = new_dir.join("settings.json");
        std::fs::copy(&old_settings, &new_settings)
            .map_err(|e| format!("copy settings.json: {}", e))?;
        eprintln!("[migration] copied settings.json");
    }

    // Copy recordings/ subfolder if it exists (forward compat).
    let old_recordings = old_dir.join("recordings");
    if old_recordings.exists() && old_recordings.is_dir() {
        let new_recordings = new_dir.join("recordings");
        copy_dir_recursive(&old_recordings, &new_recordings)
            .map_err(|e| format!("copy recordings dir: {}", e))?;
        eprintln!("[migration] copied recordings/ subfolder");
    }

    Ok(())
}

fn migrate_temp_state() -> Result<(), String> {
    let temp_root = std::env::temp_dir();
    let old_dir = temp_root.join(OLD_TEMP_SEGMENT);
    let new_dir = temp_root.join(NEW_TEMP_SEGMENT);

    let old_state_file = old_dir.join("recording-state.json");
    let new_state_file = new_dir.join("recording-state.json");

    if !old_state_file.exists() {
        return Ok(());
    }
    if new_state_file.exists() {
        return Ok(());
    }

    eprintln!(
        "[migration] migrating temp state from {} to {}",
        old_state_file.display(),
        new_state_file.display()
    );

    std::fs::create_dir_all(&new_dir).map_err(|e| format!("create new temp dir: {}", e))?;
    std::fs::rename(&old_state_file, &new_state_file)
        .or_else(|_| {
            // Fall back to copy + delete if rename fails (cross-device).
            std::fs::copy(&old_state_file, &new_state_file)
                .map(|_| ())
                .and_then(|_| std::fs::remove_file(&old_state_file))
        })
        .map_err(|e| format!("move state file: {}", e))?;

    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
