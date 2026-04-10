use tokio::process::Command;

use crate::commands::recording::resolve_ffmpeg_path;

#[cfg(target_os = "windows")]
fn apply_no_window(cmd: &mut Command) {
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
}

#[cfg(not(target_os = "windows"))]
fn apply_no_window(_cmd: &mut Command) {}

/// Records 3 seconds of audio and checks if it's non-silent.
#[tauri::command]
pub async fn test_audio(app: tauri::AppHandle, device: String) -> Result<bool, String> {
    let ffmpeg_path = resolve_ffmpeg_path(&app)?;

    if device.is_empty() || device == "none" {
        return Err("No audio device selected".to_string());
    }

    let test_dir = std::env::temp_dir().join("prospertogether");
    std::fs::create_dir_all(&test_dir).map_err(|e| e.to_string())?;
    let wav_path = test_dir.join("audio_test.wav");
    let wav_str = wav_path.to_string_lossy().to_string();

    // Record 3 seconds of audio
    let mut record_cmd = Command::new(&ffmpeg_path);
    record_cmd.args([
        "-f",
        "dshow",
        "-i",
        &format!("audio={}", device),
        "-t",
        "3",
        "-ar",
        "16000",
        "-ac",
        "1",
        "-y",
        &wav_str,
    ]);
    apply_no_window(&mut record_cmd);

    let output = record_cmd
        .output()
        .await
        .map_err(|e| format!("Audio test failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let _ = tokio::fs::remove_file(&wav_path).await;
        return Err(format!("Failed to capture audio: {}", stderr.lines().last().unwrap_or("unknown error")));
    }

    // Check if the WAV file has any signal (non-silent)
    let mut detect_cmd = Command::new(&ffmpeg_path);
    detect_cmd.args(["-i", &wav_str, "-af", "volumedetect", "-f", "null", "-"]);
    apply_no_window(&mut detect_cmd);

    let detect_output = detect_cmd.output().await.map_err(|e| e.to_string())?;

    let _ = tokio::fs::remove_file(&wav_path).await;

    let stderr = String::from_utf8_lossy(&detect_output.stderr);
    let has_signal = stderr.lines().any(|line| {
        if line.contains("mean_volume:") {
            if let Some(db_str) = line.split("mean_volume:").nth(1) {
                let db_str = db_str.trim().trim_end_matches(" dB");
                if let Ok(db) = db_str.parse::<f64>() {
                    return db > -50.0;
                }
            }
        }
        false
    });

    Ok(has_signal)
}

/// Captures a single screenshot frame via FFmpeg, returns base64 data URL.
#[tauri::command]
pub async fn capture_screenshot(app: tauri::AppHandle) -> Result<String, String> {
    use base64::Engine;

    let ffmpeg_path = resolve_ffmpeg_path(&app)?;

    let test_dir = std::env::temp_dir().join("prospertogether");
    std::fs::create_dir_all(&test_dir).map_err(|e| e.to_string())?;
    let screenshot_path = test_dir.join("screenshot_test.png");
    let screenshot_str = screenshot_path.to_string_lossy().to_string();

    let mut cmd = Command::new(&ffmpeg_path);
    cmd.args([
        "-f",
        "gdigrab",
        "-framerate",
        "1",
        "-i",
        "desktop",
        "-frames:v",
        "1",
        "-y",
        &screenshot_str,
    ]);
    apply_no_window(&mut cmd);

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("Screenshot capture failed: {}", e))?;

    if !output.status.success() {
        return Err("Failed to capture screenshot".to_string());
    }

    if !screenshot_path.exists() {
        return Err("Screenshot file was not created".to_string());
    }

    // Read file and encode as base64 data URL
    let bytes = tokio::fs::read(&screenshot_path)
        .await
        .map_err(|e| format!("Failed to read screenshot: {}", e))?;
    let _ = tokio::fs::remove_file(&screenshot_path).await;

    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:image/png;base64,{}", b64))
}
