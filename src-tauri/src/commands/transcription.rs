use tauri::{Manager, State};
use tokio::process::Command;

use crate::commands::recording::resolve_ffmpeg_path;
use crate::state::AppState;

fn resolve_whisper(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    crate::commands::recording::resolve_sidecar_path(app, "whisper-x86_64-pc-windows-msvc.exe")
}

fn resolve_model(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    // 1. Tauri resource dir (production)
    if let Ok(resource_dir) = app.path().resource_dir() {
        let path = resource_dir.join("resources").join("models").join("ggml-base.en.bin");
        if path.exists() {
            return Ok(path);
        }
    }
    // 2. Next to exe (dev copy)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let path = exe_dir.join("resources").join("models").join("ggml-base.en.bin");
            if path.exists() {
                return Ok(path);
            }
        }
    }
    // 3. Source tree (dev mode)
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let src_path = manifest.join("resources").join("models").join("ggml-base.en.bin");
    if src_path.exists() {
        return Ok(src_path);
    }
    Err("Whisper model (ggml-base.en.bin) not found. Run scripts/download-sidecars.sh first.".to_string())
}

#[cfg(target_os = "windows")]
fn apply_no_window(cmd: &mut Command) {
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
}

#[cfg(not(target_os = "windows"))]
fn apply_no_window(_cmd: &mut Command) {}

#[tauri::command]
pub async fn transcribe_audio(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    audio_path: String,
) -> Result<String, String> {
    {
        let mut active = state.transcription_active.lock().map_err(|e| e.to_string())?;
        *active = true;
    }

    let result = do_transcribe(&app, &audio_path).await;

    {
        let mut active = state.transcription_active.lock().map_err(|e| e.to_string())?;
        *active = false;
    }

    result
}

async fn do_transcribe(app: &tauri::AppHandle, video_path: &str) -> Result<String, String> {
    let ffmpeg_path = resolve_ffmpeg_path(app)?;
    let whisper_path = resolve_whisper(app)?;
    let model_path = resolve_model(app)?;

    // Step 1: Extract audio from video → 16kHz mono WAV
    let wav_path = format!("{}.wav", video_path);

    let mut extract_cmd = Command::new(&ffmpeg_path);
    extract_cmd.args([
        "-i",
        video_path,
        "-vn",
        "-ar",
        "16000",
        "-ac",
        "1",
        "-f",
        "wav",
        "-y",
        &wav_path,
    ]);
    apply_no_window(&mut extract_cmd);

    let extract_output = extract_cmd
        .output()
        .await
        .map_err(|e| format!("FFmpeg audio extraction failed: {}", e))?;

    if !extract_output.status.success() {
        let stderr = String::from_utf8_lossy(&extract_output.stderr);
        // Clean up
        let _ = tokio::fs::remove_file(&wav_path).await;
        return Err(format!("FFmpeg audio extraction failed: {}", stderr));
    }

    // Step 2: Run whisper.cpp on the WAV file
    let mut whisper_cmd = Command::new(&whisper_path);
    whisper_cmd.args([
        "-m",
        &model_path.to_string_lossy(),
        "-f",
        &wav_path,
        "--no-timestamps",
        "--no-prints",
    ]);
    apply_no_window(&mut whisper_cmd);

    let whisper_output = whisper_cmd
        .output()
        .await
        .map_err(|e| {
            format!(
                "whisper.cpp spawn failed: {} (exe={}, model={}, wav={})",
                e,
                whisper_path.display(),
                model_path.display(),
                wav_path
            )
        })?;

    if !whisper_output.status.success() {
        let stderr = String::from_utf8_lossy(&whisper_output.stderr);
        let stdout = String::from_utf8_lossy(&whisper_output.stdout);
        let code = whisper_output
            .status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "<no exit code>".to_string());
        // Intentionally do NOT delete wav_path here so the user can inspect/replay
        // the failing input. It will be cleaned up on the next successful run.
        return Err(format!(
            "whisper.cpp failed (exit {}): exe={} | model={} | wav={} | stderr={:?} | stdout={:?}",
            code,
            whisper_path.display(),
            model_path.display(),
            wav_path,
            stderr.trim(),
            stdout.trim()
        ));
    }

    // Clean up WAV file (only on success)
    let _ = tokio::fs::remove_file(&wav_path).await;

    // whisper.cpp writes transcript to stdout
    let transcript = String::from_utf8_lossy(&whisper_output.stdout)
        .trim()
        .to_string();

    // Also check for .txt output file (some versions write to file instead)
    if transcript.is_empty() {
        let txt_path = format!("{}.txt", wav_path);
        if let Ok(contents) = tokio::fs::read_to_string(&txt_path).await {
            let _ = tokio::fs::remove_file(&txt_path).await;
            return Ok(contents.trim().to_string());
        }
    }

    Ok(transcript)
}
