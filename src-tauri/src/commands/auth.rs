#[tauri::command]
pub async fn get_token(_app: tauri::AppHandle) -> Result<Option<String>, String> {
    // Token is managed by tauri-plugin-store from the frontend
    // This command exists as a fallback
    Ok(None)
}

#[tauri::command]
pub async fn save_token(_app: tauri::AppHandle, _token: String) -> Result<(), String> {
    // Token is managed by tauri-plugin-store from the frontend
    Ok(())
}

#[tauri::command]
pub async fn clear_token(_app: tauri::AppHandle) -> Result<(), String> {
    // Token is managed by tauri-plugin-store from the frontend
    Ok(())
}
