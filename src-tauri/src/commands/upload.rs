use serde::Serialize;
use tauri::State;
use tokio::fs::File;
use tokio::io::AsyncReadExt;

use crate::state::AppState;

const CHUNK_SIZE: usize = 5 * 1024 * 1024; // 5 MB

#[derive(Serialize)]
pub struct UploadResult {
    pub success: bool,
    pub recording_id: Option<String>,
}

#[tauri::command]
pub async fn init_upload(
    state: State<'_, AppState>,
    api_url: String,
    token: String,
    file_name: String,
    file_size: u64,
) -> Result<serde_json::Value, String> {
    {
        let mut active = state.upload_active.lock().map_err(|e| e.to_string())?;
        *active = true;
    }

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/recordings/upload/init", api_url))
        .header("Authorization", format!("Bearer {}", token))
        .json(&serde_json::json!({
            "originalFileName": file_name,
            "mimeType": "video/webm",
            "fileSize": file_size
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let body = response.json::<serde_json::Value>().await.map_err(|e| e.to_string())?;
    Ok(body)
}

#[tauri::command]
pub async fn upload_file(
    state: State<'_, AppState>,
    api_url: String,
    token: String,
    upload_token: String,
    file_path: String,
    duration_seconds: Option<u32>,
    transcription: Option<String>,
    frames: Option<Vec<String>>,
) -> Result<UploadResult, String> {
    let mut file = File::open(&file_path).await.map_err(|e| e.to_string())?;
    let mut buffer = vec![0u8; CHUNK_SIZE];
    let mut chunk_index: u32 = 0;
    let client = reqwest::Client::new();

    loop {
        let bytes_read = file.read(&mut buffer).await.map_err(|e| e.to_string())?;
        if bytes_read == 0 {
            break;
        }

        let chunk_data = &buffer[..bytes_read];

        let response = client
            .post(format!("{}/recordings/upload/chunk", api_url))
            .header("Authorization", format!("Bearer {}", token))
            .header("x-upload-token", &upload_token)
            .header("x-chunk-index", chunk_index.to_string())
            .header("Content-Type", "application/octet-stream")
            .body(chunk_data.to_vec())
            .send()
            .await
            .map_err(|e| format!("Chunk {} upload failed: {}", chunk_index, e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("Chunk {} failed with {}: {}", chunk_index, status, body));
        }

        chunk_index += 1;
    }

    // Complete the upload
    let complete_response = client
        .post(format!("{}/recordings/upload/complete", api_url))
        .header("Authorization", format!("Bearer {}", token))
        .json(&serde_json::json!({
            "uploadToken": upload_token,
            "durationSeconds": duration_seconds,
            "transcription": transcription,
            "frames": frames
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let complete_body = complete_response
        .json::<serde_json::Value>()
        .await
        .map_err(|e| e.to_string())?;

    let recording_id = complete_body["id"].as_str().map(|s| s.to_string());

    {
        let mut active = state.upload_active.lock().map_err(|e| e.to_string())?;
        *active = false;
    }

    Ok(UploadResult {
        success: true,
        recording_id,
    })
}
