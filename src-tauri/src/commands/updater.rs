use tauri::State;

use crate::state::AppState;

#[tauri::command]
pub async fn check_for_update(
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    // Check if any active operations would prevent update
    let recording = state.recording_active.lock().map_err(|e| e.to_string())?;
    let uploading = state.upload_active.lock().map_err(|e| e.to_string())?;
    let transcribing = state.transcription_active.lock().map_err(|e| e.to_string())?;

    let can_update = !*recording && !*uploading && !*transcribing;

    Ok(serde_json::json!({
        "canUpdate": can_update,
        "recording": *recording,
        "uploading": *uploading,
        "transcribing": *transcribing
    }))
}
