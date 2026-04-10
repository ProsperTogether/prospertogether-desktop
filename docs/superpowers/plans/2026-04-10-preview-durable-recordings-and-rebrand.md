# Preview, Durable Recordings, and Rebrand Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebrand the desktop app from "UserFirst Agent" to "Prosper Together" AND add a post-recording review screen backed by durable storage so users can preview, submit, or delete recordings without risk of data loss.

**Architecture:** Two tracks. Track A is a mechanical rename (config files, Rust source, frontend UI strings, bundle identifier migration helper). Track B moves finished recordings from `%TEMP%` into a self-contained folder under `{app_data_dir}/recordings/{uuid}/` (video + thumbnail + metadata.json), adds a `/review/:id` page that runs the transcribe→upload pipeline only on explicit Submit, and shows a `/recordings` pending list on app startup.

**Tech Stack:** Rust (Tauri 2 backend), existing `windows-capture` + `ffmpeg` sidecar, React 18 + TypeScript + Tailwind, Zustand (existing), React Router 6.

**Working directory:** `C:/Code/happier/portal/agent/`

**Not a git repository** — per-task verification is `cargo check` (not `git commit`). Do NOT attempt to run `git` commands.

---

## File Structure

### Created

**Rust backend:**
- `src-tauri/src/commands/recordings.rs` — New module: `RecordingMeta` type, list/get/delete/path commands, thumbnail generation helper, move-to-durable-storage helper
- `src-tauri/src/migration.rs` — Bundle identifier migration (copies old app data + temp state file on first launch after rename)

**Frontend:**
- `src/types/recording.ts` — TypeScript `RecordingMeta` type matching the Rust struct
- `src/hooks/useSubmitRecording.ts` — Hook that runs transcribe → keyframes → upload → delete pipeline
- `src/components/recording/ReviewPage.tsx` — Post-recording preview screen
- `src/components/recording/RecordingsListPage.tsx` — Pending recordings list

### Modified

**Rust:**
- `src-tauri/Cargo.toml` — Package + lib name rename
- `src-tauri/tauri.conf.json` — productName, identifier, window title, asset protocol config
- `src-tauri/capabilities/default.json` — Identifier if referenced
- `src-tauri/src/lib.rs` — Tray tooltip, register new module and commands, call migration
- `src-tauri/src/commands/mod.rs` — Register `recordings` module
- `src-tauri/src/commands/recording.rs` — Temp path strings, stop_recording finalization, return type
- `src-tauri/src/commands/window_capture.rs` — Temp path strings
- `src-tauri/src/commands/setup.rs` — Temp path strings

**Frontend:**
- `package.json` — name field
- `src/App.tsx` — New routes `/review/:id` and `/recordings`
- `src/components/recording/RecordingControls.tsx` — handleStop simplification, remove processing/uploading UI branches
- `src/components/dashboard/DashboardPage.tsx` — Pending recordings badge
- `src/components/layout/AppLayout.tsx` — UI strings
- `src/components/auth/LoginPage.tsx` — UI strings
- `src/components/onboarding/OnboardingPage.tsx` — UI strings
- `src/store/recordingStore.ts` — Remove unused `processing` and `uploading` status variants IF they become unused (check before removing)

### Untouched

- `src-tauri/src/commands/transcription.rs`, `upload.rs`, `frames.rs`, `auth.rs`, `devices.rs`, `updater.rs` — existing submission pipeline stays as-is, just called from a new location
- `src-tauri/src/capture_target.rs`, `state.rs` — unchanged
- All window picker / region picker / border overlay code — unchanged

---

# Track A: Rename to Prosper Together

## Task 1: Rename Cargo.toml package and lib

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: Update `[package]` and `[lib]` sections**

Current:
```toml
[package]
name = "userfirst-agent"
version = "1.0.0"
description = "UserFirst Screen Recording Agent"
authors = ["UserFirst"]
edition = "2021"

[lib]
name = "userfirst_agent_lib"
crate-type = ["lib", "cdylib", "staticlib"]
```

New:
```toml
[package]
name = "prospertogether-desktop"
version = "1.0.0"
description = "Prosper Together Screen Recording Desktop App"
authors = ["Prosper Together"]
edition = "2021"

[lib]
name = "prospertogether_desktop_lib"
crate-type = ["lib", "cdylib", "staticlib"]
```

- [ ] **Step 2: Do NOT run cargo check yet**

Cargo check will fail because the binary references still use the old lib name. Task 7 (`cargo clean` + full rebuild) runs after all rename tasks.

---

## Task 2: Rename package.json

**Files:**
- Modify: `package.json`

- [ ] **Step 1: Update the `name` field**

Change the `"name"` field value from whatever it currently is (likely `"userfirst-agent"` or `"agent"`) to `"prospertogether-desktop"`. Leave other fields (`version`, `scripts`, `dependencies`, etc.) untouched.

- [ ] **Step 2: No verification needed**

package.json renames don't affect builds immediately. Vite and Tauri pick up the change on next start.

---

## Task 3: Update tauri.conf.json

**Files:**
- Modify: `src-tauri/tauri.conf.json`

- [ ] **Step 1: Read the current file**

Find: `productName`, `identifier`, and any `title` field under `app.windows[].title`.

- [ ] **Step 2: Update the rename fields**

- `productName`: `"UserFirst Agent"` → `"Prosper Together"`
- `identifier`: `"com.userfirst.agent"` → `"com.prospertogether.desktop"`
- `app.windows[0].title`: `"UserFirst Agent"` → `"Prosper Together"`

Preserve all other fields exactly as they are.

- [ ] **Step 3: Add asset protocol configuration**

In the `app.security` section (create it if absent), add:

```jsonc
"assetProtocol": {
  "enable": true,
  "scope": [
    "$APPDATA/com.prospertogether.desktop/recordings/**"
  ]
}
```

If `app.security` already exists and has other keys (e.g., `csp`), merge the `assetProtocol` key in without touching the existing ones.

- [ ] **Step 4: No verification yet**

This will be verified in Task 7 (clean rebuild).

---

## Task 4: Update capabilities/default.json

**Files:**
- Modify: `src-tauri/capabilities/default.json`

- [ ] **Step 1: Check for bundle identifier references**

Read the current file. If `"com.userfirst.agent"` appears anywhere (typically in `identifier` or `permissions` sections), replace with `"com.prospertogether.desktop"`. If the file does not reference the identifier at all, note that and move on.

- [ ] **Step 2: Add asset protocol permissions if needed**

For HTML5 video to load local files via `convertFileSrc`, the capability file may need an allowlist entry. Inspect the current `permissions` array. If it contains `"core:default"` or similar broad grants, nothing more is needed (the asset protocol is configured in tauri.conf.json and doesn't require an extra permission in capabilities).

If the plan executor finds a narrow permissions list, add `"core:asset:default"` or the closest equivalent. Report DONE_WITH_CONCERNS if unsure.

---

## Task 5: Rename Rust source paths (temp directory)

**Files:**
- Modify: `src-tauri/src/commands/recording.rs`
- Modify: `src-tauri/src/commands/window_capture.rs`
- Modify: `src-tauri/src/commands/setup.rs`

- [ ] **Step 1: Find all `"userfirst"` string literals in recording.rs**

Search `src-tauri/src/commands/recording.rs` for the string `"userfirst"` (case-sensitive). In the current code it appears at least in:
- `state_file_path()` → `.join("userfirst")`
- Any `output_dir` computation like `std::env::temp_dir().join("userfirst")`

Replace each `"userfirst"` with `"prospertogether"`. Do not change function names, variable names, or comments — only the string literal used in path construction.

- [ ] **Step 2: Same in window_capture.rs**

Search `src-tauri/src/commands/window_capture.rs` for `"userfirst"` and replace with `"prospertogether"`.

- [ ] **Step 3: Same in setup.rs**

Search `src-tauri/src/commands/setup.rs` for `"userfirst"` and replace with `"prospertogether"`.

- [ ] **Step 4: No cargo check yet**

Verified in Task 7.

---

## Task 6: Rename Rust tray tooltip and any hardcoded strings in lib.rs

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Find `"UserFirst Agent"` string**

Search for `"UserFirst Agent"` in `src-tauri/src/lib.rs`. In the tray icon builder section, there is a line like:

```rust
let _tray = TrayIconBuilder::new()
    .menu(&menu)
    .tooltip("UserFirst Agent")
```

Change `"UserFirst Agent"` to `"Prosper Together"`.

- [ ] **Step 2: Find any other UserFirst references**

Case-insensitive search in `src-tauri/src/lib.rs` for `userfirst`. Replace any remaining occurrences:
- String literals in tray menus or notifications
- Do NOT change variable names, type names, or comments

- [ ] **Step 3: No cargo check yet**

---

## Task 7: Rename frontend UI strings

**Files:**
- Modify: `src/components/layout/AppLayout.tsx`
- Modify: `src/components/auth/LoginPage.tsx`
- Modify: `src/components/onboarding/OnboardingPage.tsx`

- [ ] **Step 1: Replace in AppLayout.tsx**

Open the file. Find occurrences of `"UserFirst"` or `"UserFirst Agent"` in JSX text nodes, alt text, titles, and aria labels. Replace with `"Prosper Together"`. Do NOT touch component names, import paths, or prop names.

- [ ] **Step 2: Replace in LoginPage.tsx**

Same treatment. Find user-visible text only (JSX text, placeholders, button labels). Common patterns:
- `"Sign in to UserFirst"` → `"Sign in to Prosper Together"`
- `"UserFirst Agent"` heading → `"Prosper Together"`
- Email body previews like `"Your UserFirst Agent login code"` are sent by the server; don't change here. Only change what the frontend renders.

- [ ] **Step 3: Replace in OnboardingPage.tsx**

Same treatment. Welcome headings, body copy, button labels.

- [ ] **Step 4: Verify with grep**

Run: `grep -rn "UserFirst" src/components/layout/AppLayout.tsx src/components/auth/LoginPage.tsx src/components/onboarding/OnboardingPage.tsx`

Expected: no matches (or only import paths that shouldn't change).

---

## Task 8: Create bundle identifier migration module

**Files:**
- Create: `src-tauri/src/migration.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Create the migration module**

Create `src-tauri/src/migration.rs` with:

```rust
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

use std::path::{Path, PathBuf};

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

    // Compute the old path. On Windows, Tauri's app_data_dir is
    // %APPDATA%/Roaming/<identifier>/. We can't call app_data_dir with a
    // different identifier, so derive from the new path's parent.
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
        // Already migrated.
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

/// Suppress unused warnings for PathBuf import on non-Windows targets.
#[allow(dead_code)]
fn _unused_import_suppressor() -> PathBuf {
    PathBuf::new()
}
```

- [ ] **Step 2: Wire up the module in lib.rs**

Open `src-tauri/src/lib.rs`. At the top, near the existing `mod commands; mod state; pub mod capture_target;` declarations, add:

```rust
mod migration;
```

Then, in the `.setup(|app| { ... })` block, find where `recover_orphaned_recording` is called. Add the migration call BEFORE the recovery call (migration must run first so the recovered state file is at the new path):

```rust
        .setup(|app| {
            // Bundle-identifier migration: copy settings + orphaned state
            // from com.userfirst.agent (if it exists) to the new app data
            // dir. Safe to run on every startup; no-op when already done.
            migration::run_migration(&app.handle());

            // Recover any orphaned recording from a previous (crashed/killed)
            // Tauri process. ...
            {
                let app_state: tauri::State<AppState> = app.state();
                commands::recording::recover_orphaned_recording(app_state.inner());
            }
            // ... rest of existing setup ...
```

- [ ] **Step 3: No cargo check yet**

---

## Task 9: cargo clean + verify full rebuild

**Files:**
- None modified — verification only

- [ ] **Step 1: Clean the build cache**

Run: `cd C:/Code/happier/portal/agent/src-tauri && cargo clean`

Expected: `Removed ... files ... bytes` or similar. No errors.

- [ ] **Step 2: Run cargo check**

Run: `cd C:/Code/happier/portal/agent/src-tauri && cargo check`

Expected: the crate compiles cleanly under the new package name `prospertogether-desktop`. Warnings are acceptable (`dead_code`, `unused_imports`). Errors are NOT acceptable — if any appear, they indicate a miss in Tasks 1-8.

- [ ] **Step 3: Grep for any remaining UserFirst references**

Run: `grep -rn -i "userfirst" src-tauri/src/ src/ src-tauri/tauri.conf.json src-tauri/Cargo.toml package.json 2>/dev/null || true`

Expected: zero matches in any file under `src-tauri/src/`, `src/`, or top-level config. Matches in `target/` (if any survived the clean) or `node_modules/` are allowed but not expected.

If there are remaining matches in application files, fix them, re-run cargo check, and re-run grep until clean.

---

# Track B: Durable Recordings + Preview Screen

## Task 10: Create RecordingMeta type and recordings path helper

**Files:**
- Create: `src-tauri/src/commands/recordings.rs`
- Modify: `src-tauri/src/commands/mod.rs`

- [ ] **Step 1: Create the recordings module with types and path helpers**

Create `src-tauri/src/commands/recordings.rs` with:

```rust
//! Durable recording storage and management.
//!
//! Each pending recording is a self-contained folder under
//! `{app_data_dir}/recordings/{uuid}/` containing:
//!   - `recording.webm`    — the captured video
//!   - `thumbnail.jpg`     — single frame for preview cards
//!   - `metadata.json`     — duration, capture target, size, timestamps
//!
//! This layout has no central index file: listing is just `readdir` +
//! parse each `metadata.json`. Delete is `remove_dir_all` of one folder.
//! If the app crashes between writing the video and writing the metadata,
//! the folder can be salvaged by `recover_incomplete_recordings` at startup.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

pub const RECORDING_VIDEO_FILENAME: &str = "recording.webm";
pub const RECORDING_THUMBNAIL_FILENAME: &str = "thumbnail.jpg";
pub const RECORDING_METADATA_FILENAME: &str = "metadata.json";

/// Minimal capture target label — just enough to display in the review
/// screen. Original hwnd/rect are NOT persisted (they'd be stale anyway).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum PersistedCaptureTarget {
    Screen,
    Monitor { label: String },
    Window { title: String },
    Region { width: u32, height: u32 },
}

/// Metadata stored alongside each recording. Matches the TypeScript
/// `RecordingMeta` type in `src/types/recording.ts`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingMeta {
    pub id: String,
    pub created_at: String, // ISO-8601
    pub duration_seconds: u64,
    pub file_size_bytes: u64,
    pub video_file_name: String,
    pub thumbnail_file_name: Option<String>,
    pub capture_target: PersistedCaptureTarget,
}

/// Return `{app_data_dir}/recordings/`, creating it if missing.
pub fn recordings_root(app: &AppHandle) -> Result<PathBuf, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir failed: {}", e))?;
    let recordings = app_data.join("recordings");
    std::fs::create_dir_all(&recordings)
        .map_err(|e| format!("create recordings dir: {}", e))?;
    Ok(recordings)
}

/// Return `{recordings_root}/{id}/`, creating it if missing.
pub fn recording_folder(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    let root = recordings_root(app)?;
    let folder = root.join(id);
    std::fs::create_dir_all(&folder)
        .map_err(|e| format!("create recording folder: {}", e))?;
    Ok(folder)
}
```

- [ ] **Step 2: Register the module**

Open `src-tauri/src/commands/mod.rs` and add:

```rust
pub mod recordings;
```

- [ ] **Step 3: Verify compile**

Run: `cargo check --manifest-path C:/Code/happier/portal/agent/src-tauri/Cargo.toml`

Expected: compiles cleanly. `dead_code` warnings on the new types are expected — they'll be used in later tasks.

---

## Task 11: Thumbnail generation helper

**Files:**
- Modify: `src-tauri/src/commands/recordings.rs`

- [ ] **Step 1: Add the thumbnail helper**

Append to `src-tauri/src/commands/recordings.rs`:

```rust
use std::process::Stdio;
use tokio::process::Command;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
fn apply_no_window(cmd: &mut Command) {
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
}

#[cfg(not(target_os = "windows"))]
fn apply_no_window(_cmd: &mut Command) {}

/// Extract a single thumbnail frame from the given video file using the
/// bundled ffmpeg sidecar. Writes to `{folder}/thumbnail.jpg` scaled to
/// max 640px wide. Returns `Ok(())` on success, `Err(msg)` on failure.
///
/// Non-fatal: a failed thumbnail should not block the recording save.
/// The caller should log the error and continue without a thumbnail
/// (the frontend will show a placeholder).
pub async fn generate_thumbnail(
    app: &AppHandle,
    video_path: &std::path::Path,
    folder: &std::path::Path,
    duration_seconds: u64,
) -> Result<(), String> {
    let ffmpeg_path = crate::commands::recording::resolve_ffmpeg_path(app)?;
    let thumbnail_path = folder.join(RECORDING_THUMBNAIL_FILENAME);

    // Seek to 2s in, or to frame 0 for shorter recordings.
    let seek_pos = if duration_seconds >= 2 { "00:00:02" } else { "00:00:00" };

    let mut cmd = Command::new(&ffmpeg_path);
    cmd.args([
        "-ss",
        seek_pos,
        "-i",
    ])
    .arg(video_path)
    .args([
        "-vframes",
        "1",
        "-vf",
        "scale='min(640,iw)':-2",
        "-q:v",
        "5",
        "-y",
    ])
    .arg(&thumbnail_path)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null());
    apply_no_window(&mut cmd);

    let status = cmd
        .status()
        .await
        .map_err(|e| format!("spawn ffmpeg (thumbnail): {}", e))?;

    if !status.success() {
        return Err(format!(
            "ffmpeg thumbnail exited with status {:?}",
            status.code()
        ));
    }

    if !thumbnail_path.exists() {
        return Err("ffmpeg ran successfully but thumbnail file is missing".to_string());
    }

    Ok(())
}
```

- [ ] **Step 2: Verify compile**

Run: `cargo check --manifest-path C:/Code/happier/portal/agent/src-tauri/Cargo.toml`

Expected: compiles cleanly. Dead-code warnings on the helper are expected.

---

## Task 12: Finalize-recording helper (move temp → durable + write metadata)

**Files:**
- Modify: `src-tauri/src/commands/recordings.rs`

- [ ] **Step 1: Add finalize_recording function**

Append to `src-tauri/src/commands/recordings.rs`:

```rust
use std::time::SystemTime;

/// Move a completed temp-file recording into its durable folder and write
/// metadata.json alongside it. Called by `stop_recording` after ffmpeg has
/// finalized the webm and (if applicable) segment concat has completed.
///
/// On success, returns the recording ID (uuid string). The temp file is
/// deleted. On failure, the temp file is NOT deleted — caller should
/// surface the error so the user doesn't silently lose a recording.
pub async fn finalize_recording(
    app: &AppHandle,
    temp_file_path: &std::path::Path,
    duration_seconds: u64,
    capture_target: PersistedCaptureTarget,
) -> Result<RecordingMeta, String> {
    if !temp_file_path.exists() {
        return Err(format!(
            "temp recording file does not exist: {}",
            temp_file_path.display()
        ));
    }

    let id = uuid::Uuid::new_v4().to_string();
    let folder = recording_folder(app, &id)?;
    let video_dst = folder.join(RECORDING_VIDEO_FILENAME);

    // Move the file. Use rename first (atomic on same filesystem); fall
    // back to copy+delete if rename fails with EXDEV (cross-device).
    match std::fs::rename(temp_file_path, &video_dst) {
        Ok(()) => {}
        Err(_) => {
            std::fs::copy(temp_file_path, &video_dst)
                .map_err(|e| format!("copy temp → durable: {}", e))?;
            let _ = std::fs::remove_file(temp_file_path);
        }
    }

    // Read file size after the move.
    let file_size_bytes = std::fs::metadata(&video_dst)
        .map_err(|e| format!("stat video file: {}", e))?
        .len();

    // Generate thumbnail (non-fatal).
    let thumbnail_file_name = match generate_thumbnail(app, &video_dst, &folder, duration_seconds).await {
        Ok(()) => Some(RECORDING_THUMBNAIL_FILENAME.to_string()),
        Err(e) => {
            eprintln!("[recordings] thumbnail generation failed (non-fatal): {}", e);
            None
        }
    };

    // ISO-8601 timestamp using a format without extra deps.
    let created_at = iso8601_now();

    let meta = RecordingMeta {
        id: id.clone(),
        created_at,
        duration_seconds,
        file_size_bytes,
        video_file_name: RECORDING_VIDEO_FILENAME.to_string(),
        thumbnail_file_name,
        capture_target,
    };

    // Write metadata.json.
    let meta_json = serde_json::to_string_pretty(&meta)
        .map_err(|e| format!("serialize metadata: {}", e))?;
    let meta_path = folder.join(RECORDING_METADATA_FILENAME);
    std::fs::write(&meta_path, meta_json)
        .map_err(|e| format!("write metadata.json: {}", e))?;

    Ok(meta)
}

/// Format the current system time as an ISO-8601 UTC string (e.g.
/// `2026-04-10T14:23:45.123Z`). We avoid pulling in `chrono` for this
/// one use by computing the components manually from UNIX_EPOCH.
fn iso8601_now() -> String {
    let now = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = now.as_secs();
    let millis = now.subsec_millis();

    // Days since 1970-01-01
    let days = total_secs / 86400;
    let secs_of_day = total_secs % 86400;
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;

    // Convert days to year/month/day (civil algorithm from Howard Hinnant).
    let (year, month, day) = civil_from_days(days as i64);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        year, month, day, hour, minute, second, millis
    )
}

/// Convert days since 1970-01-01 to (year, month, day). Algorithm from
/// Howard Hinnant's chrono paper — exact, handles leap years correctly.
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
```

- [ ] **Step 2: Verify compile**

Run: `cargo check --manifest-path C:/Code/happier/portal/agent/src-tauri/Cargo.toml`

Expected: compiles cleanly. Dead-code warnings are expected until later tasks wire this up.

---

## Task 13: list/get/delete/path Tauri commands

**Files:**
- Modify: `src-tauri/src/commands/recordings.rs`

- [ ] **Step 1: Add the commands**

Append to `src-tauri/src/commands/recordings.rs`:

```rust
/// List all pending recordings in `{app_data_dir}/recordings/`, sorted
/// newest-first by createdAt. Folders with unparseable or missing
/// metadata.json get a "Recovered recording" placeholder so the user can
/// still see and delete them — never silently drop files the user created.
#[tauri::command]
pub async fn list_pending_recordings(app: AppHandle) -> Result<Vec<RecordingMeta>, String> {
    let root = recordings_root(&app)?;
    let mut out: Vec<RecordingMeta> = Vec::new();

    let entries = match std::fs::read_dir(&root) {
        Ok(e) => e,
        Err(_) => return Ok(out),
    };

    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let folder = entry.path();
        let id = match entry.file_name().to_str() {
            Some(s) => s.to_string(),
            None => continue,
        };

        let meta_path = folder.join(RECORDING_METADATA_FILENAME);
        let meta: Option<RecordingMeta> = std::fs::read_to_string(&meta_path)
            .ok()
            .and_then(|s| serde_json::from_str::<RecordingMeta>(&s).ok());

        if let Some(m) = meta {
            out.push(m);
            continue;
        }

        // Recovery: metadata missing or corrupted. Generate a placeholder
        // using whatever info we can recover from the filesystem.
        let video_path = folder.join(RECORDING_VIDEO_FILENAME);
        let file_size_bytes = std::fs::metadata(&video_path).map(|m| m.len()).unwrap_or(0);
        let thumbnail_exists = folder.join(RECORDING_THUMBNAIL_FILENAME).exists();
        let created_at = iso8601_now(); // best effort — real time unknown

        out.push(RecordingMeta {
            id,
            created_at,
            duration_seconds: 0,
            file_size_bytes,
            video_file_name: RECORDING_VIDEO_FILENAME.to_string(),
            thumbnail_file_name: if thumbnail_exists {
                Some(RECORDING_THUMBNAIL_FILENAME.to_string())
            } else {
                None
            },
            capture_target: PersistedCaptureTarget::Screen,
        });
    }

    // Sort newest first by created_at (lexicographic on ISO-8601 = chronological).
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(out)
}

#[tauri::command]
pub async fn get_recording(app: AppHandle, id: String) -> Result<RecordingMeta, String> {
    let folder = recordings_root(&app)?.join(&id);
    let meta_path = folder.join(RECORDING_METADATA_FILENAME);
    let s = std::fs::read_to_string(&meta_path)
        .map_err(|e| format!("read metadata: {}", e))?;
    serde_json::from_str::<RecordingMeta>(&s)
        .map_err(|e| format!("parse metadata: {}", e))
}

#[tauri::command]
pub async fn get_recording_video_path(app: AppHandle, id: String) -> Result<String, String> {
    let folder = recordings_root(&app)?.join(&id);
    let video = folder.join(RECORDING_VIDEO_FILENAME);
    if !video.exists() {
        return Err(format!("video file not found for recording {}", id));
    }
    Ok(video.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn get_recording_thumbnail_path(app: AppHandle, id: String) -> Result<String, String> {
    let folder = recordings_root(&app)?.join(&id);
    let thumb = folder.join(RECORDING_THUMBNAIL_FILENAME);
    if !thumb.exists() {
        return Err(format!("thumbnail not found for recording {}", id));
    }
    Ok(thumb.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn delete_recording(app: AppHandle, id: String) -> Result<(), String> {
    let folder = recordings_root(&app)?.join(&id);
    if !folder.exists() {
        return Err(format!("recording {} not found", id));
    }
    std::fs::remove_dir_all(&folder)
        .map_err(|e| format!("remove recording folder: {}", e))?;
    Ok(())
}
```

- [ ] **Step 2: Verify compile**

Run: `cargo check --manifest-path C:/Code/happier/portal/agent/src-tauri/Cargo.toml`

Expected: compiles cleanly. All five commands are currently dead code — they get registered in Task 15.

---

## Task 14: Modify stop_recording to finalize into durable storage

**Files:**
- Modify: `src-tauri/src/commands/recording.rs`

- [ ] **Step 1: Read the current `stop_recording` function**

Locate `pub async fn stop_recording` in `src-tauri/src/commands/recording.rs`. Note:
- Its current return type is `Result<String, String>` returning the file path
- It calls `stop_ffmpeg`, handles segment concat (if multiple segments), clears state, and returns `recording_path`
- It takes `app: tauri::AppHandle` and `state: State<'_, AppState>` (added for concat)

- [ ] **Step 2: Compute duration before stopping**

Inside `stop_recording`, immediately after the early-return check for `recording_active` being false, add:

```rust
    // Compute duration before we clear `recording_started_at_ms` below.
    // stop_recording returns a RecordingMeta to the frontend which needs
    // this field. If started_at_ms is None (recovered recording without a
    // timestamp), default to 0 — the user can still play the file.
    let computed_duration_seconds: u64 = {
        let started_at = state
            .recording_started_at_ms
            .lock()
            .map_err(|e| e.to_string())?
            .unwrap_or(0);
        if started_at == 0 {
            0
        } else {
            let now = now_unix_ms();
            if now > started_at {
                (now - started_at) / 1000
            } else {
                0
            }
        }
    };
```

- [ ] **Step 3: Change the return type and add finalize call at the end**

Find the current end of `stop_recording` which looks like:

```rust
    Ok(recording_path)
}
```

Replace the return block — starting from the `// Clear recording state` comment and going through `Ok(recording_path)` — with:

```rust
    // Clear recording state
    {
        let mut active = state.recording_active.lock().map_err(|e| e.to_string())?;
        *active = false;
    }
    {
        let mut s = state
            .recording_started_at_ms
            .lock()
            .map_err(|e| e.to_string())?;
        *s = None;
    }
    {
        let mut path = state
            .current_recording_path
            .lock()
            .map_err(|e| e.to_string())?;
        *path = None;
    }
    {
        *state.segment_index.lock().map_err(|e| e.to_string())? = 0;
    }
    delete_state_file();

    // Recording file is still at `recording_path` (a temp dir path).
    // Move it to the durable `{app_data_dir}/recordings/{uuid}/` folder
    // and write metadata. The frontend will navigate to /review/{id} to
    // preview before committing to the upload.
    if recording_path.is_empty() {
        return Err("stop_recording: no output path available".to_string());
    }
    let temp_path = std::path::PathBuf::from(&recording_path);

    // We don't persist the original CaptureTarget inside stop_recording's
    // scope, so use a generic Screen placeholder. The review screen's
    // metadata display will show "Recording" as the label. A future
    // improvement: thread the capture target through AppState so we can
    // persist the actual mode + title.
    let capture_target =
        crate::commands::recordings::PersistedCaptureTarget::Screen;

    let meta = crate::commands::recordings::finalize_recording(
        &app,
        &temp_path,
        computed_duration_seconds,
        capture_target,
    )
    .await?;

    Ok(meta)
}
```

Change the function signature's return type from `Result<String, String>` to `Result<crate::commands::recordings::RecordingMeta, String>`.

- [ ] **Step 4: Verify compile**

Run: `cargo check --manifest-path C:/Code/happier/portal/agent/src-tauri/Cargo.toml`

Expected: compiles cleanly OR fails with errors in callers of `stop_recording` that expected the String return. The only caller is the frontend via IPC — Rust compilation should succeed. If there's a `#[tauri::command]` wrapper that infers types, it should work automatically because the new return type is serde-serializable.

---

## Task 15: Register new commands in lib.rs

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add the five new commands to the invoke_handler**

Find the `tauri::generate_handler![...]` macro invocation in `src-tauri/src/lib.rs`. Add after the existing `commands::devices::list_windows` line (or anywhere in the list):

```rust
            commands::recordings::list_pending_recordings,
            commands::recordings::get_recording,
            commands::recordings::get_recording_video_path,
            commands::recordings::get_recording_thumbnail_path,
            commands::recordings::delete_recording,
```

Make sure each line has a trailing comma to match the list style.

- [ ] **Step 2: Verify compile**

Run: `cargo check --manifest-path C:/Code/happier/portal/agent/src-tauri/Cargo.toml`

Expected: compiles cleanly. The `dead_code` warnings on the recording commands from earlier tasks should disappear. `finalize_recording` and `generate_thumbnail` may still show `dead_code` until stop_recording is actually invoked.

---

## Task 16: Frontend RecordingMeta type

**Files:**
- Create: `src/types/recording.ts`

- [ ] **Step 1: Create the types file**

Create `src/types/recording.ts` with:

```typescript
/**
 * Minimal capture target label used when persisting a recording. Does not
 * include HWND or rect — those would be stale by the time the user opens
 * the review screen.
 */
export type PersistedCaptureTarget =
  | { mode: 'screen' }
  | { mode: 'monitor'; label: string }
  | { mode: 'window'; title: string }
  | { mode: 'region'; width: number; height: number };

/**
 * Metadata for a single pending recording. Mirrors the Rust `RecordingMeta`
 * struct in `src-tauri/src/commands/recordings.rs`.
 *
 * IMPORTANT: serde uses snake_case field names when serializing. Keep the
 * property names below in snake_case to match (do NOT convert to camelCase
 * here — the wire format is snake_case). The UI code that consumes these
 * objects accesses `created_at`, `duration_seconds`, etc.
 */
export interface RecordingMeta {
  id: string;
  created_at: string; // ISO-8601
  duration_seconds: number;
  file_size_bytes: number;
  video_file_name: string;
  thumbnail_file_name: string | null;
  capture_target: PersistedCaptureTarget;
}
```

- [ ] **Step 2: No compile step**

TypeScript types compile on the next vite rebuild.

---

## Task 17: useSubmitRecording hook

**Files:**
- Create: `src/hooks/useSubmitRecording.ts`

- [ ] **Step 1: Create the hook**

Create `src/hooks/useSubmitRecording.ts` with:

```typescript
import { useState, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';

export type SubmitStage =
  | 'idle'
  | 'transcribing'
  | 'keyframes'
  | 'uploading'
  | 'success'
  | 'error';

export interface SubmitResult {
  success: boolean;
  serverRecordingId: string | null;
  error?: string;
}

/**
 * Runs the transcribe → keyframes → upload pipeline for a single pending
 * recording. On success, deletes the local recording folder. On any
 * failure, leaves the recording in place so the user can retry. The
 * pipeline is not abortable mid-flight — the caller should disable the
 * Submit button while `stage` is not 'idle' / 'success' / 'error'.
 *
 * This hook replaces the inline pipeline that previously lived in
 * `RecordingControls.handleStop`. Extracting it keeps the review screen's
 * logic declarative and makes the stages individually visible to the UI.
 */
export function useSubmitRecording() {
  const [stage, setStage] = useState<SubmitStage>('idle');
  const [error, setError] = useState<string | null>(null);

  const submit = useCallback(async (recordingId: string, durationSeconds: number): Promise<SubmitResult> => {
    setError(null);
    setStage('transcribing');

    try {
      // 1. Get the absolute path of the video file.
      const videoPath = await invoke<string>('get_recording_video_path', { id: recordingId });

      // 2. Transcribe audio (non-blocking on failure — we still upload).
      let transcript = '';
      try {
        transcript = await invoke<string>('transcribe_audio', {
          audioPath: videoPath,
        });
      } catch (err) {
        console.error('[useSubmitRecording] transcription failed:', err);
        // Non-fatal — continue to frames + upload.
      }

      // 3. Extract keyframes (non-blocking on failure).
      setStage('keyframes');
      let frames: string[] | null = null;
      try {
        frames = await invoke<string[]>('extract_keyframes', {
          videoPath,
          durationSeconds: durationSeconds > 0 ? durationSeconds : null,
        });
      } catch (err) {
        console.warn('[useSubmitRecording] keyframe extraction failed:', err);
      }

      // 4. Upload to portal.
      setStage('uploading');

      const { load } = await import('@tauri-apps/plugin-store');
      const store = await load('settings.json');
      const token = (await store.get<string>('auth_token')) ?? '';
      const apiUrl =
        import.meta.env.VITE_API_BASE_URL ?? 'http://localhost:4000/api';

      const fs = await import('@tauri-apps/plugin-fs');
      const stat = await fs.stat(videoPath);
      const fileSize = stat.size;

      const initResult = await invoke<{
        id: string;
        uploadToken: string;
        chunkSize: number;
      }>('init_upload', {
        apiUrl,
        token,
        fileName: videoPath.split(/[\\/]/).pop() ?? 'recording.webm',
        fileSize,
      });

      const uploadResult = await invoke<{
        success: boolean;
        recordingId: string | null;
      }>('upload_file', {
        apiUrl,
        token,
        uploadToken: initResult.uploadToken,
        filePath: videoPath,
        durationSeconds,
        transcription: transcript || null,
        frames: frames && frames.length > 0 ? frames : null,
      });

      if (!uploadResult.success) {
        throw new Error('Upload returned success=false');
      }

      // 5. Delete the local recording folder now that the server has it.
      try {
        await invoke('delete_recording', { id: recordingId });
      } catch (err) {
        // Non-fatal — upload succeeded, just log the cleanup failure.
        console.warn('[useSubmitRecording] delete_recording failed:', err);
      }

      setStage('success');
      return { success: true, serverRecordingId: uploadResult.recordingId };
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      console.error('[useSubmitRecording] submit failed:', msg);
      setError(msg);
      setStage('error');
      return { success: false, serverRecordingId: null, error: msg };
    }
  }, []);

  const reset = useCallback(() => {
    setStage('idle');
    setError(null);
  }, []);

  return { stage, error, submit, reset };
}
```

- [ ] **Step 2: No compile step**

---

## Task 18: ReviewPage component

**Files:**
- Create: `src/components/recording/ReviewPage.tsx`

- [ ] **Step 1: Create the component**

Create `src/components/recording/ReviewPage.tsx` with:

```tsx
import { useState, useEffect, useCallback } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { invoke, convertFileSrc } from '@tauri-apps/api/core';

import { useSubmitRecording, type SubmitStage } from '../../hooks/useSubmitRecording';
import type { RecordingMeta } from '../../types/recording';

function formatDuration(seconds: number): string {
  const m = Math.floor(seconds / 60).toString().padStart(2, '0');
  const s = Math.floor(seconds % 60).toString().padStart(2, '0');
  return `${m}:${s}`;
}

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

function formatRelativeTime(isoString: string): string {
  const then = new Date(isoString).getTime();
  const now = Date.now();
  const diffSec = Math.floor((now - then) / 1000);
  if (diffSec < 60) return 'just now';
  if (diffSec < 3600) return `${Math.floor(diffSec / 60)} minute${Math.floor(diffSec / 60) === 1 ? '' : 's'} ago`;
  if (diffSec < 86400) return `${Math.floor(diffSec / 3600)} hour${Math.floor(diffSec / 3600) === 1 ? '' : 's'} ago`;
  return `${Math.floor(diffSec / 86400)} day${Math.floor(diffSec / 86400) === 1 ? '' : 's'} ago`;
}

function captureTargetLabel(target: RecordingMeta['capture_target']): string {
  switch (target.mode) {
    case 'screen':
      return 'Entire Screen';
    case 'monitor':
      return target.label;
    case 'window':
      return target.title;
    case 'region':
      return `Region ${target.width}×${target.height}`;
  }
}

function stageLabel(stage: SubmitStage): string {
  switch (stage) {
    case 'transcribing':
      return 'Transcribing audio…';
    case 'keyframes':
      return 'Extracting keyframes…';
    case 'uploading':
      return 'Uploading to server…';
    case 'success':
      return 'Upload complete';
    case 'error':
      return 'Upload failed';
    default:
      return '';
  }
}

export const ReviewPage = () => {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();

  const [meta, setMeta] = useState<RecordingMeta | null>(null);
  const [videoSrc, setVideoSrc] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  const { stage, error: submitError, submit } = useSubmitRecording();
  const busy = stage !== 'idle' && stage !== 'error' && stage !== 'success';

  // Load metadata + video src on mount or when id changes.
  useEffect(() => {
    if (!id) return;
    let cancelled = false;
    (async () => {
      try {
        const m = await invoke<RecordingMeta>('get_recording', { id });
        if (cancelled) return;
        setMeta(m);
        const path = await invoke<string>('get_recording_video_path', { id });
        if (cancelled) return;
        setVideoSrc(convertFileSrc(path));
      } catch (err) {
        if (cancelled) return;
        setLoadError(err instanceof Error ? err.message : String(err));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [id]);

  const handleSubmit = useCallback(async () => {
    if (!meta) return;
    const result = await submit(meta.id, meta.duration_seconds);
    if (result.success) {
      if (result.serverRecordingId) {
        navigate(`/submit?recordingId=${result.serverRecordingId}`);
      } else {
        navigate('/submit');
      }
    }
    // On failure, stay on this page — error banner is rendered below.
  }, [meta, submit, navigate]);

  const handleRecordAgain = useCallback(async () => {
    if (!meta) return;
    const ok = window.confirm('Delete this recording and start a new one?');
    if (!ok) return;
    try {
      await invoke('delete_recording', { id: meta.id });
    } catch (err) {
      console.warn('[ReviewPage] delete failed:', err);
    }
    navigate('/record');
  }, [meta, navigate]);

  const handleDelete = useCallback(async () => {
    if (!meta) return;
    const ok = window.confirm('Delete this recording? This cannot be undone.');
    if (!ok) return;
    try {
      await invoke('delete_recording', { id: meta.id });
    } catch (err) {
      console.warn('[ReviewPage] delete failed:', err);
    }
    // Go to recordings list if there may be other pending; dashboard if empty.
    try {
      const remaining = await invoke<RecordingMeta[]>('list_pending_recordings');
      navigate(remaining.length > 0 ? '/recordings' : '/');
    } catch {
      navigate('/');
    }
  }, [meta, navigate]);

  if (loadError) {
    return (
      <div className="flex flex-col items-center justify-center py-16 px-6">
        <p className="text-lg font-medium text-slate-900 mb-2">Recording not found</p>
        <p className="text-sm text-slate-500 mb-6">{loadError}</p>
        <button
          onClick={() => navigate('/recordings')}
          className="px-4 py-2 bg-slate-900 text-white text-sm font-medium rounded-lg"
        >
          Back to recordings
        </button>
      </div>
    );
  }

  if (!meta || !videoSrc) {
    return (
      <div className="flex items-center justify-center py-16">
        <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-brand-500 to-brand-700 animate-pulse" />
      </div>
    );
  }

  return (
    <div className="flex flex-col items-center py-8 px-6">
      <div className="w-full max-w-2xl">
        <h2 className="text-xl font-semibold text-slate-900 mb-1">Review Recording</h2>
        <p className="text-[13px] text-slate-500 mb-5">
          Watch your capture before sending it in.
        </p>

        {submitError && (
          <div className="rounded-lg bg-red-50 border border-red-100 px-3.5 py-2.5 mb-4">
            <p className="text-[13px] text-red-600">Upload failed: {submitError}</p>
          </div>
        )}

        {/* Video player */}
        <div className="rounded-xl overflow-hidden bg-black shadow-lg mb-4">
          <video
            src={videoSrc}
            controls
            className="w-full h-auto block"
            style={{ maxHeight: '60vh' }}
          />
        </div>

        {/* Metadata */}
        <div className="grid grid-cols-2 gap-3 mb-5 text-[13px]">
          <div className="rounded-lg border border-slate-200 px-3 py-2">
            <div className="text-[11px] text-slate-400 uppercase tracking-wide">Duration</div>
            <div className="text-slate-900 font-medium">{formatDuration(meta.duration_seconds)}</div>
          </div>
          <div className="rounded-lg border border-slate-200 px-3 py-2">
            <div className="text-[11px] text-slate-400 uppercase tracking-wide">Size</div>
            <div className="text-slate-900 font-medium">{formatFileSize(meta.file_size_bytes)}</div>
          </div>
          <div className="rounded-lg border border-slate-200 px-3 py-2">
            <div className="text-[11px] text-slate-400 uppercase tracking-wide">Recorded</div>
            <div className="text-slate-900 font-medium">{formatRelativeTime(meta.created_at)}</div>
          </div>
          <div className="rounded-lg border border-slate-200 px-3 py-2">
            <div className="text-[11px] text-slate-400 uppercase tracking-wide">Target</div>
            <div className="text-slate-900 font-medium truncate">{captureTargetLabel(meta.capture_target)}</div>
          </div>
        </div>

        {/* Submission stage indicator */}
        {busy && (
          <div className="rounded-lg bg-blue-50 border border-blue-100 px-3.5 py-2.5 mb-4 flex items-center gap-3">
            <div className="w-4 h-4 border-2 border-blue-300 border-t-blue-600 rounded-full animate-spin" />
            <p className="text-[13px] text-blue-700">{stageLabel(stage)}</p>
          </div>
        )}

        {/* Actions */}
        <div className="flex flex-col gap-2">
          <button
            onClick={handleSubmit}
            disabled={busy}
            className="w-full px-6 py-3 bg-gradient-to-b from-red-500 to-red-600 text-white text-[15px] font-semibold rounded-xl hover:from-red-600 hover:to-red-700 shadow-lg shadow-red-500/25 transition disabled:opacity-50 disabled:cursor-not-allowed"
          >
            Submit
          </button>
          <div className="grid grid-cols-2 gap-2">
            <button
              onClick={handleRecordAgain}
              disabled={busy}
              className="px-4 py-2.5 bg-white text-slate-700 text-[13px] font-medium rounded-lg border border-slate-200 hover:bg-slate-50 transition disabled:opacity-50 disabled:cursor-not-allowed"
            >
              Record Again
            </button>
            <button
              onClick={handleDelete}
              disabled={busy}
              className="px-4 py-2.5 bg-white text-red-600 text-[13px] font-medium rounded-lg border border-red-200 hover:bg-red-50 transition disabled:opacity-50 disabled:cursor-not-allowed"
            >
              Delete
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
```

- [ ] **Step 2: No compile step**

---

## Task 19: RecordingsListPage component

**Files:**
- Create: `src/components/recording/RecordingsListPage.tsx`

- [ ] **Step 1: Create the component**

Create `src/components/recording/RecordingsListPage.tsx` with:

```tsx
import { useState, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { invoke, convertFileSrc } from '@tauri-apps/api/core';

import type { RecordingMeta } from '../../types/recording';

function formatDuration(seconds: number): string {
  const m = Math.floor(seconds / 60).toString().padStart(2, '0');
  const s = Math.floor(seconds % 60).toString().padStart(2, '0');
  return `${m}:${s}`;
}

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

function formatRelativeTime(isoString: string): string {
  const then = new Date(isoString).getTime();
  const now = Date.now();
  const diffSec = Math.floor((now - then) / 1000);
  if (diffSec < 60) return 'just now';
  if (diffSec < 3600) return `${Math.floor(diffSec / 60)}m ago`;
  if (diffSec < 86400) return `${Math.floor(diffSec / 3600)}h ago`;
  return `${Math.floor(diffSec / 86400)}d ago`;
}

function captureTargetLabel(target: RecordingMeta['capture_target']): string {
  switch (target.mode) {
    case 'screen':
      return 'Entire Screen';
    case 'monitor':
      return target.label;
    case 'window':
      return target.title;
    case 'region':
      return `Region ${target.width}×${target.height}`;
  }
}

interface CardProps {
  recording: RecordingMeta;
  onOpen: (id: string) => void;
}

const RecordingCard = ({ recording, onOpen }: CardProps) => {
  const [thumbSrc, setThumbSrc] = useState<string | null>(null);

  useEffect(() => {
    if (!recording.thumbnail_file_name) return;
    let cancelled = false;
    (async () => {
      try {
        const path = await invoke<string>('get_recording_thumbnail_path', { id: recording.id });
        if (!cancelled) setThumbSrc(convertFileSrc(path));
      } catch {
        // no thumbnail — placeholder shows
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [recording.id, recording.thumbnail_file_name]);

  return (
    <button
      onClick={() => onOpen(recording.id)}
      className="flex flex-col rounded-xl border border-slate-200 overflow-hidden text-left bg-white hover:border-slate-300 hover:shadow-sm transition"
    >
      <div className="w-full aspect-video bg-slate-100 flex items-center justify-center">
        {thumbSrc ? (
          <img src={thumbSrc} alt="" className="w-full h-full object-cover" draggable={false} />
        ) : (
          <svg className="w-10 h-10 text-slate-300" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
            <rect x="3" y="5" width="18" height="14" rx="2" />
            <path d="M10 9l5 3-5 3V9z" fill="currentColor" />
          </svg>
        )}
      </div>
      <div className="p-3">
        <p className="text-[13px] font-medium text-slate-900 truncate mb-0.5">
          {captureTargetLabel(recording.capture_target)}
        </p>
        <p className="text-[11px] text-slate-500">
          {formatDuration(recording.duration_seconds)} · {formatFileSize(recording.file_size_bytes)} · {formatRelativeTime(recording.created_at)}
        </p>
      </div>
    </button>
  );
};

export const RecordingsListPage = () => {
  const navigate = useNavigate();
  const [recordings, setRecordings] = useState<RecordingMeta[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = () => {
    setLoading(true);
    invoke<RecordingMeta[]>('list_pending_recordings')
      .then((list) => {
        setRecordings(list);
        setError(null);
      })
      .catch((err) => setError(err instanceof Error ? err.message : String(err)))
      .finally(() => setLoading(false));
  };

  useEffect(() => {
    load();
  }, []);

  const handleOpen = (id: string) => {
    navigate(`/review/${id}`);
  };

  return (
    <div className="flex flex-col py-8 px-6">
      <div className="w-full max-w-4xl mx-auto">
        <div className="flex items-center justify-between mb-5">
          <div>
            <h2 className="text-xl font-semibold text-slate-900 mb-1">Pending Recordings</h2>
            <p className="text-[13px] text-slate-500">
              Review, submit, or delete recordings you haven't sent in yet.
            </p>
          </div>
          <button
            onClick={() => navigate('/record')}
            className="px-4 py-2 bg-slate-900 text-white text-[13px] font-medium rounded-lg hover:bg-slate-800 transition"
          >
            New Recording
          </button>
        </div>

        {error && (
          <div className="rounded-lg bg-red-50 border border-red-100 px-3.5 py-2.5 mb-4">
            <p className="text-[13px] text-red-600">{error}</p>
          </div>
        )}

        {loading ? (
          <div className="flex items-center justify-center py-16">
            <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-brand-500 to-brand-700 animate-pulse" />
          </div>
        ) : recordings.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-16 text-center">
            <p className="text-slate-900 font-medium mb-1">No pending recordings</p>
            <p className="text-[13px] text-slate-500 mb-5">Start a new recording to see it here.</p>
          </div>
        ) : (
          <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 gap-4">
            {recordings.map((r) => (
              <RecordingCard key={r.id} recording={r} onOpen={handleOpen} />
            ))}
          </div>
        )}
      </div>
    </div>
  );
};
```

- [ ] **Step 2: No compile step**

---

## Task 20: Wire up routes in App.tsx

**Files:**
- Modify: `src/App.tsx`

- [ ] **Step 1: Add route imports**

Near the existing imports in `src/App.tsx`, add:

```tsx
import { ReviewPage } from './components/recording/ReviewPage';
import { RecordingsListPage } from './components/recording/RecordingsListPage';
```

- [ ] **Step 2: Add the routes**

Find the `<Routes>` block. Add these two routes inside it, alongside the existing `/record` and `/submit` routes:

```tsx
        <Route path="/review/:id" element={<AuthedLayout><ReviewPage /></AuthedLayout>} />
        <Route path="/recordings" element={<AuthedLayout><RecordingsListPage /></AuthedLayout>} />
```

- [ ] **Step 3: No compile step**

Vite picks up the change on next reload.

---

## Task 21: Simplify RecordingControls.handleStop and remove processing/uploading UI

**Files:**
- Modify: `src/components/recording/RecordingControls.tsx`

- [ ] **Step 1: Replace the body of handleStop**

Find the `handleStop` useCallback in `RecordingControls.tsx`. The current body runs stop_recording → transcribe → keyframes → upload → delete → navigate. Replace the entire body with:

```tsx
  const handleStop = useCallback(async () => {
    // Destroy border overlay if any
    try {
      const { destroyBorderOverlay } = await import('../../overlay/useBorderOverlay');
      await destroyBorderOverlay();
    } catch { /* may not exist */ }

    // Stop watching window rect if any
    try {
      const core = await import('@tauri-apps/api/core');
      await core.invoke('stop_watching_window');
    } catch { /* may not be watching */ }

    // Destroy drawing overlay while still recording so final strokes capture
    try {
      const { destroyOverlayWindow } = await import('../../overlay/useOverlayWindow');
      await destroyOverlayWindow();
    } catch { /* overlay may not exist */ }
    setDrawingActive(false);

    // Restore the main window from dock to its normal size/position BEFORE
    // we navigate. Otherwise the ReviewPage renders inside the 84px dock.
    await restoreWindow();

    setError(null);
    setStartedAtMs(null);

    try {
      const core = await import('@tauri-apps/api/core');
      const meta = await core.invoke<{ id: string }>('stop_recording');
      // Reset local store state before navigating.
      useRecordingStore.getState().reset();
      navigate(`/review/${meta.id}`);
    } catch (err) {
      setError(`Stop failed: ${err}`);
      // Reset so the user can try again from the idle screen.
      useRecordingStore.getState().reset();
    }
  }, [navigate, restoreWindow, setStartedAtMs]);
```

- [ ] **Step 2: Remove the processing / uploading render branches**

Find the JSX blocks that handle `if (status === 'processing' || status === 'uploading')`. These branches render the spinner + "Processing / Uploading" text. Delete the entire block — after this change, `RecordingControls` never enters those states because navigation to `/review/:id` happens before.

- [ ] **Step 3: Check recordingStore.ts for unused status variants**

Open `src/store/recordingStore.ts`. Look at the `RecordingStatus` type. If `'processing'` and `'uploading'` are still listed but no code anywhere sets them, remove them from the union. If grep finds any remaining references, leave them alone (TypeScript will complain at compile time about removed variants being referenced).

Run: `grep -rn "'processing'" src/ | grep -v node_modules` and `grep -rn "'uploading'" src/ | grep -v node_modules`

If both return only the type definition in `recordingStore.ts`, remove those variants. Otherwise leave them.

- [ ] **Step 4: Verify frontend doesn't reference removed imports**

Specifically check that `RecordingControls.tsx` no longer imports any transcription/upload helpers, since the pipeline moved to `useSubmitRecording`. The imports that can be removed:
- Anything specific to `init_upload`, `upload_file`, `transcribe_audio`, `extract_keyframes` — these were inline `invoke` calls, not top-level imports, so likely nothing to remove. Just double-check.

---

## Task 22: Dashboard badge for pending recordings

**Files:**
- Modify: `src/components/dashboard/DashboardPage.tsx`

- [ ] **Step 1: Read the current DashboardPage**

Open `src/components/dashboard/DashboardPage.tsx`. Note its current structure (it likely has cards for starting a new recording, account info, etc.).

- [ ] **Step 2: Add pending-count fetch**

At the top of the component function body, add:

```tsx
import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

// ... inside the component:
  const [pendingCount, setPendingCount] = useState(0);

  useEffect(() => {
    invoke<Array<{ id: string }>>('list_pending_recordings')
      .then((list) => setPendingCount(list.length))
      .catch(() => setPendingCount(0));
  }, []);
```

Adapt the existing imports as needed — `useState` and `useEffect` may already be imported. If `invoke` is not imported in the file, add it.

- [ ] **Step 3: Add a "Pending Recordings" card**

Find wherever the dashboard renders its action cards (or the main body). Add a new card that routes to `/recordings`:

```tsx
  import { useNavigate } from 'react-router-dom';
  // ...
  const navigate = useNavigate();
  // ...
  <button
    onClick={() => navigate('/recordings')}
    className="flex items-center justify-between w-full px-4 py-3 rounded-xl border border-slate-200 bg-white hover:border-slate-300 hover:shadow-sm transition text-left"
  >
    <div>
      <div className="text-[14px] font-medium text-slate-900">Pending Recordings</div>
      <div className="text-[12px] text-slate-500">
        {pendingCount > 0
          ? `${pendingCount} recording${pendingCount === 1 ? '' : 's'} awaiting review`
          : 'No pending recordings'}
      </div>
    </div>
    {pendingCount > 0 && (
      <span className="inline-flex items-center justify-center min-w-[24px] h-6 px-2 rounded-full bg-red-500 text-white text-[11px] font-semibold">
        {pendingCount}
      </span>
    )}
  </button>
```

Place this card in a reasonable spot — typically above or beside the "New Recording" action. Use existing styling classes as a guide.

- [ ] **Step 4: No compile step**

---

## Task 23: Full rebuild verification

**Files:**
- None modified — verification only

- [ ] **Step 1: Run cargo check**

Run: `cd C:/Code/happier/portal/agent/src-tauri && cargo check`

Expected: compiles cleanly. Dead-code warnings are acceptable. Errors are not.

- [ ] **Step 2: Start tauri dev**

Run: `cd C:/Code/happier/portal/agent && npm run tauri dev`

Wait for the full Rust rebuild and Vite to serve. Expected: the app window opens titled "Prosper Together".

- [ ] **Step 3: Smoke test the rename**

Verify in the running app:
- Main window title is "Prosper Together"
- Login page says "Prosper Together" (not "UserFirst")
- Onboarding page says "Prosper Together"
- Tray icon tooltip says "Prosper Together"
- If you were previously logged in with the old bundle ID, you should still be logged in (the migration copied settings.json). If you see a login screen, the migration did not run or did not find the old path — report and investigate.

- [ ] **Step 4: Smoke test the preview flow**

1. Click "New Recording" → Ready-to-Record screen shows
2. Click Start Recording, record 3–5 seconds, click Stop
3. Expected: navigate to `/review/{id}` with video player, metadata panel, Submit / Record Again / Delete buttons
4. Click Play on the video: it should play back. If video does not load (black rectangle), the asset protocol scope is wrong — check `tauri.conf.json`.
5. Click Submit: progress states show (Transcribing → Keyframes → Uploading → success), then navigate to `/submit`
6. Verify the `{app_data_dir}/recordings/` folder is empty after success (or does not contain the submitted recording)

- [ ] **Step 5: Smoke test the durability**

1. Start a new recording, stop, arrive at /review, do NOT click submit
2. Close the app (close the main window and any dock)
3. Relaunch via `npm run tauri dev`
4. Expected: dashboard shows a "Pending Recordings" badge with count = 1
5. Click through → /recordings list shows the recording
6. Click the card → review screen loads with video playable
7. Click Submit → upload succeeds → recording folder deleted

- [ ] **Step 6: Smoke test Record Again / Delete**

1. Record a short clip, land on /review
2. Click "Record Again" → confirm → back on /record screen
3. Verify the previous recording folder is gone
4. Repeat: record, click Delete → confirm → navigate out
5. Verify deletion

---

## Self-Review Notes

**Spec coverage:**
- Track A rename: Tasks 1-9 (Cargo.toml, package.json, tauri.conf.json, capabilities, Rust paths, tray tooltip, frontend strings, migration module, clean rebuild)
- Durable storage: Tasks 10-15 (types, path helper, thumbnail gen, finalize helper, CRUD commands, stop_recording rewrite, command registration)
- Preview screen + submit pipeline: Tasks 16-21 (TS types, hook, ReviewPage, RecordingsListPage, routes, RecordingControls cleanup)
- Pending count surface: Task 22 (dashboard badge)
- Verification: Task 23

**Placeholder scan:** clean — every code block is complete. No "add error handling" or "TBD" markers.

**Type consistency:**
- `RecordingMeta` fields use snake_case in both Rust and TS (explicitly documented in the TS file to prevent future camelCase drift)
- `PersistedCaptureTarget` discriminated union is identical in both
- `SubmitStage` values match the hook's setState calls
- `delete_recording`, `get_recording`, `get_recording_video_path`, `get_recording_thumbnail_path`, `list_pending_recordings` command names match between Rust registration and TS invoke calls

**Known simplifications deliberately taken:**
- `stop_recording` persists `PersistedCaptureTarget::Screen` regardless of actual target. A future improvement threads the real capture target into AppState so finalize can use it. Documented in Task 14 Step 3.
- No background submission that outlives the review screen (matches spec decision).
- ISO-8601 formatting is done manually to avoid pulling in `chrono` just for timestamps.

**Out of scope** (per spec): auto-cleanup, multi-select delete, video editing, cross-device sync.
