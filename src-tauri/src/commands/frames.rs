use base64::{engine::general_purpose, Engine as _};
use tokio::process::Command;

use crate::commands::recording::resolve_ffmpeg_path;

#[cfg(target_os = "windows")]
fn apply_no_window(cmd: &mut Command) {
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
}

#[cfg(not(target_os = "windows"))]
fn apply_no_window(_cmd: &mut Command) {}

/// Extract evenly-spaced keyframes from a recorded video using the bundled ffmpeg
/// binary. Returns base64-encoded JPEG data so the renderer can include them in the
/// upload-complete payload without touching the filesystem from JS.
///
/// The frames are sampled at `duration_seconds / target_frame_count` intervals (with
/// a hard cap), scaled to 1280px wide, and encoded at JPEG quality 4 (high quality but
/// modest size — typically 50–200 KB per frame).
///
/// On any error, returns the error string so the frontend can surface it. The caller
/// is expected to swallow this error and proceed with upload (analysis will fall back
/// to transcript-only).
#[tauri::command]
pub async fn extract_keyframes(
    app: tauri::AppHandle,
    video_path: String,
    duration_seconds: Option<f64>,
) -> Result<Vec<String>, String> {
    let ffmpeg_path = resolve_ffmpeg_path(&app)?;

    // Decide how many frames and how wide an interval. Cap at 12 frames.
    let target_frames: u32 = 10;
    let max_frames: u32 = 12;
    let interval_seconds: f64 = match duration_seconds {
        Some(d) if d > 0.0 => (d / target_frames as f64).max(1.0),
        _ => 6.0, // unknown duration: ~10 frames over a 60s recording
    };

    // Output frames to a sibling temp folder named <video>.frames/
    let video_path_buf = std::path::PathBuf::from(&video_path);
    let frames_dir = video_path_buf.with_extension("frames");
    if frames_dir.exists() {
        // Clean any leftover from a prior run.
        let _ = tokio::fs::remove_dir_all(&frames_dir).await;
    }
    tokio::fs::create_dir_all(&frames_dir)
        .await
        .map_err(|e| format!("Cannot create frames dir: {}", e))?;

    let pattern = frames_dir.join("frame_%03d.jpg");
    let pattern_str = pattern.to_string_lossy().to_string();
    let vf_arg = format!("fps=1/{:.3},scale=1280:-1", interval_seconds);

    let mut cmd = Command::new(&ffmpeg_path);
    cmd.args([
        "-i",
        &video_path,
        "-vf",
        &vf_arg,
        "-frames:v",
        &max_frames.to_string(),
        "-q:v",
        "4",
        "-y",
        &pattern_str,
    ]);
    apply_no_window(&mut cmd);

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("FFmpeg keyframe extraction failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let _ = tokio::fs::remove_dir_all(&frames_dir).await;
        return Err(format!("FFmpeg keyframe extraction failed: {}", stderr));
    }

    // Read each frame, base64-encode, then delete on the way out.
    let mut frames_b64: Vec<String> = Vec::new();
    let mut entries = tokio::fs::read_dir(&frames_dir)
        .await
        .map_err(|e| format!("Cannot read frames dir: {}", e))?;

    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("jpg") {
            paths.push(path);
        }
    }
    paths.sort();

    for path in &paths {
        match tokio::fs::read(path).await {
            Ok(bytes) => {
                let encoded = general_purpose::STANDARD.encode(&bytes);
                frames_b64.push(encoded);
            }
            Err(e) => {
                eprintln!("[extract_keyframes] failed to read {:?}: {}", path, e);
            }
        }
    }

    // Clean up temp dir
    let _ = tokio::fs::remove_dir_all(&frames_dir).await;

    Ok(frames_b64)
}
