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
//! the folder can be salvaged by `list_pending_recordings` at listing
//! time (it generates placeholder metadata for any folder with a video
//! but no valid metadata.json).

use std::path::PathBuf;
use std::process::Stdio;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tokio::process::Command;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

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

/// Convert a live `CaptureTarget` (which carries hwnd/rect used at runtime)
/// into a `PersistedCaptureTarget` (which just carries a display label).
/// Used when persisting metadata so the review screen shows the actual
/// target the user picked, not a generic "Entire Screen" placeholder.
pub fn persisted_target_from(live: &crate::capture_target::CaptureTarget) -> PersistedCaptureTarget {
    use crate::capture_target::CaptureTarget;
    match live {
        CaptureTarget::Screen => PersistedCaptureTarget::Screen,
        CaptureTarget::Monitor { width, height, .. } => PersistedCaptureTarget::Monitor {
            label: format!("Monitor {}×{}", width, height),
        },
        CaptureTarget::Window { title, .. } => PersistedCaptureTarget::Window {
            title: title.clone(),
        },
        CaptureTarget::Region { width, height, .. } => PersistedCaptureTarget::Region {
            width: *width,
            height: *height,
        },
    }
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
/// Non-fatal at the caller level: a failed thumbnail should not block
/// the recording save. The caller logs the error and continues without
/// a thumbnail (the frontend shows a placeholder).
pub async fn generate_thumbnail(
    app: &AppHandle,
    video_path: &std::path::Path,
    folder: &std::path::Path,
    duration_seconds: u64,
) -> Result<(), String> {
    let ffmpeg_path = crate::commands::recording::resolve_ffmpeg_path(app)?;
    let thumbnail_path = folder.join(RECORDING_THUMBNAIL_FILENAME);

    // Seek to 2s in, or to frame 0 for shorter recordings.
    let seek_pos = if duration_seconds >= 2 {
        "00:00:02"
    } else {
        "00:00:00"
    };

    let mut cmd = Command::new(&ffmpeg_path);
    cmd.args(["-ss", seek_pos, "-i"])
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

/// Move a completed temp-file recording into its durable folder and write
/// metadata.json alongside it. Called by `stop_recording` after ffmpeg has
/// finalized the webm and (if applicable) segment concat has completed.
///
/// On success, the temp file is deleted. On failure, the temp file is
/// NOT deleted — caller should surface the error so the user doesn't
/// silently lose a recording.
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
    let thumbnail_file_name =
        match generate_thumbnail(app, &video_dst, &folder, duration_seconds).await {
            Ok(()) => Some(RECORDING_THUMBNAIL_FILENAME.to_string()),
            Err(e) => {
                eprintln!(
                    "[recordings] thumbnail generation failed (non-fatal): {}",
                    e
                );
                None
            }
        };

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

    let days = total_secs / 86400;
    let secs_of_day = total_secs % 86400;
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;

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

/// List all pending recordings in `{app_data_dir}/recordings/`, sorted
/// newest-first by createdAt. Folders with unparseable or missing
/// metadata.json get a "Recovered recording" placeholder so the user
/// can still see and delete them — never silently drop files the user
/// created.
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
        if !video_path.exists() {
            // No video at all — skip this folder entirely.
            continue;
        }
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
    let s = std::fs::read_to_string(&meta_path).map_err(|e| format!("read metadata: {}", e))?;
    serde_json::from_str::<RecordingMeta>(&s).map_err(|e| format!("parse metadata: {}", e))
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
pub async fn get_recording_thumbnail_path(
    app: AppHandle,
    id: String,
) -> Result<String, String> {
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
    std::fs::remove_dir_all(&folder).map_err(|e| format!("remove recording folder: {}", e))?;
    Ok(())
}
