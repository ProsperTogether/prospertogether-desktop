use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State};
use tokio::io::AsyncWriteExt;
use tokio::process::{ChildStdin, Command};

use crate::capture_target::CaptureTarget;
use crate::state::AppState;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
fn apply_no_window(cmd: &mut Command) {
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
}

#[cfg(not(target_os = "windows"))]
fn apply_no_window(_cmd: &mut Command) {}

// ─── Persistent recording state (survives Tauri process death) ───────────────
//
// Without disk persistence, the in-memory `AppState` is reset on every Tauri
// process start. If a previous process was killed mid-recording (Ctrl+C in
// `tauri dev`, crash, etc.) the spawned `ffmpeg.exe` is orphaned because
// Windows does not auto-kill grandchildren of the parent process. The new
// Tauri process has no idea a recording is in progress, the user starts a
// "fresh" recording, and now there are TWO ffmpegs writing audio at once.
//
// We avoid that by writing the PID + file_path + start time to a small JSON
// file every time a recording starts, and reading + verifying it on the next
// process startup. If the recorded PID is still alive we restore AppState so
// the recording can be cleanly stopped through the normal flow.

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RecordingStateFile {
    pub pid: u32,
    pub file_path: String,
    pub started_at_ms: u64,
}

#[derive(Serialize, Clone, Debug)]
pub struct RecordingStateDto {
    pub active: bool,
    pub file_path: Option<String>,
    pub started_at_ms: Option<u64>,
    pub duration_seconds: u64,
}

fn state_file_path() -> std::path::PathBuf {
    std::env::temp_dir()
        .join("prospertogether")
        .join("recording-state.json")
}

fn write_state_file(s: &RecordingStateFile) -> std::io::Result<()> {
    let path = state_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(s)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(path, json)
}

fn delete_state_file() {
    let _ = std::fs::remove_file(state_file_path());
}

fn read_state_file() -> Option<RecordingStateFile> {
    let json = std::fs::read_to_string(state_file_path()).ok()?;
    serde_json::from_str(&json).ok()
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(target_os = "windows")]
fn is_pid_alive(pid: u32) -> bool {
    let mut cmd = std::process::Command::new("tasklist");
    cmd.args(["/FI", &format!("PID eq {}", pid), "/NH", "/FO", "CSV"]);
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    match cmd.output() {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            // tasklist with /FI on a missing PID prints "INFO: No tasks..." to
            // stdout, so checking that the PID number appears anywhere is the
            // most reliable signal.
            stdout.contains(&pid.to_string())
        }
        Err(_) => false,
    }
}

#[cfg(not(target_os = "windows"))]
fn is_pid_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Called once at Tauri startup. If the on-disk state file points to an
/// `ffmpeg.exe` that is still alive, restore `AppState` so the recording can
/// be controlled from the new process. If the PID is dead the file is stale
/// and gets deleted. Note: `ffmpeg_stdin` cannot be recovered (it was a pipe
/// owned by the dead Rust process), so `stop_recording` falls back to
/// taskkill rather than the graceful "send q to stdin" path.
pub fn recover_orphaned_recording(state: &AppState) {
    let Some(file) = read_state_file() else {
        return;
    };

    if !is_pid_alive(file.pid) {
        eprintln!(
            "[recovery] stale recording-state.json (pid {} not alive), removing",
            file.pid
        );
        delete_state_file();
        return;
    }

    eprintln!(
        "[recovery] resuming orphaned recording: pid={}, file={}",
        file.pid, file.file_path
    );

    if let Ok(mut active) = state.recording_active.lock() {
        *active = true;
    }
    if let Ok(mut p) = state.ffmpeg_pid.lock() {
        *p = Some(file.pid);
    }
    if let Ok(mut path) = state.current_recording_path.lock() {
        *path = Some(file.file_path.clone());
    }
    if let Ok(mut s) = state.recording_started_at_ms.lock() {
        *s = Some(file.started_at_ms);
    }
}

fn resolve_sidecar(app: &tauri::AppHandle, name: &str) -> Result<std::path::PathBuf, String> {
    // 1. Tauri resource dir (production builds)
    if let Ok(resource_dir) = app.path().resource_dir() {
        let path = resource_dir.join("binaries").join(name);
        if path.exists() {
            return Ok(path);
        }
    }
    // 2. Next to the exe (copied during dev)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let path = exe_dir.join("binaries").join(name);
            if path.exists() {
                return Ok(path);
            }
        }
    }
    // 3. Source tree src-tauri/binaries/ (dev mode, no copy needed)
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let src_path = manifest.join("binaries").join(name);
    if src_path.exists() {
        return Ok(src_path);
    }
    Err(format!(
        "{} not found. Run scripts/download-sidecars.sh first.",
        name
    ))
}

fn resolve_ffmpeg(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    resolve_sidecar(app, "ffmpeg-x86_64-pc-windows-msvc.exe")
}

pub fn resolve_ffmpeg_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    resolve_ffmpeg(app)
}

pub fn resolve_sidecar_path(app: &tauri::AppHandle, name: &str) -> Result<std::path::PathBuf, String> {
    resolve_sidecar(app, name)
}

// ─── Sync helpers (keep MutexGuard out of async functions) ──────────────────

fn collect_and_clear_segments(state: &AppState, current_path: &str) -> Result<Vec<String>, String> {
    let mut segments = state.recording_segments.lock().map_err(|e| e.to_string())?;
    if !current_path.is_empty() {
        segments.push(current_path.to_string());
    }
    let collected = segments.clone();
    segments.clear();
    Ok(collected)
}

fn push_segment(state: &AppState, path: &str) -> Result<(), String> {
    let mut segments = state.recording_segments.lock().map_err(|e| e.to_string())?;
    segments.push(path.to_string());
    Ok(())
}

fn next_segment_index(state: &AppState) -> Result<u32, String> {
    let mut idx = state.segment_index.lock().map_err(|e| e.to_string())?;
    *idx += 1;
    Ok(*idx)
}

// ─── FFmpeg spawning helper ───────────────────────────────────────────────────

struct FfmpegResult {
    pid: Option<u32>,
    stdin: Option<ChildStdin>,
}

/// Spawn an ffmpeg process for a given capture target and audio device.
/// Returns the PID, piped stdin handle, and output file path.
async fn spawn_ffmpeg(
    ffmpeg_path: &std::path::Path,
    target: &CaptureTarget,
    audio: Option<&str>,
    output_path: &str,
) -> Result<FfmpegResult, String> {
    let mut args: Vec<String> = Vec::new();

    // Audio input FIRST (dshow must come before gdigrab for proper sync)
    let audio_device = audio.unwrap_or("");
    if !audio_device.is_empty() && audio_device != "none" {
        args.extend([
            "-f".into(),
            "dshow".into(),
            "-i".into(),
            format!("audio={}", audio_device),
        ]);
    }

    // Screen capture input based on CaptureTarget
    match target {
        CaptureTarget::Screen => {
            args.extend(
                ["-f", "gdigrab", "-framerate", "15", "-i", "desktop"]
                    .iter()
                    .map(|s| s.to_string()),
            );
        }
        CaptureTarget::Monitor {
            x,
            y,
            width,
            height,
        }
        | CaptureTarget::Region {
            x,
            y,
            width,
            height,
        } => {
            args.extend([
                "-f".to_string(),
                "gdigrab".to_string(),
                "-framerate".to_string(),
                "15".to_string(),
                "-offset_x".to_string(),
                x.to_string(),
                "-offset_y".to_string(),
                y.to_string(),
                "-video_size".to_string(),
                format!("{}x{}", width, height),
                "-i".to_string(),
                "desktop".to_string(),
            ]);
        }
        CaptureTarget::Window { .. } => {
            // Window captures go through the WGC path in window_capture.rs
            // and never reach spawn_ffmpeg. If this branch fires, the
            // dispatcher in start_recording or switch_capture_target is
            // broken — fail loud rather than silently do the wrong thing.
            return Err(
                "spawn_ffmpeg called with Window target (dispatcher bug)".to_string(),
            );
        }
    }

    // Scale down (avoid upscaling small windows) and convert pixel format
    args.extend([
        "-vf".into(),
        "scale='min(1920,iw):-2'".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-c:v".into(),
        "libvpx".into(),
        "-b:v".into(),
        "1M".into(),
        "-deadline".into(),
        "realtime".into(),
        "-cpu-used".into(),
        "8".into(),
    ]);

    if !audio_device.is_empty() && audio_device != "none" {
        args.extend(["-c:a".into(), "libopus".into()]);
    } else {
        args.extend(["-an".into()]);
    }

    args.extend(["-y".into(), output_path.to_string()]);

    // Log stderr to file for diagnostics
    let output_dir = std::env::temp_dir().join("prospertogether");
    let log_path = output_dir.join("ffmpeg.log");
    let log_file = std::fs::File::create(&log_path).ok();

    let mut cmd = Command::new(ffmpeg_path);
    cmd.args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(match log_file {
            Some(f) => Stdio::from(f),
            None => Stdio::null(),
        });

    apply_no_window(&mut cmd);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start FFmpeg: {}", e))?;

    let pid = child.id();
    let stdin = child.stdin.take();

    // Wait briefly to verify FFmpeg didn't crash on startup
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
        let log = std::fs::read_to_string(&log_path).unwrap_or_default();
        let last_lines: String = log
            .lines()
            .rev()
            .take(3)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        return Err(format!(
            "FFmpeg exited immediately (code {:?}):\n{}",
            status.code(),
            last_lines
        ));
    }

    // Background task to wait for child (handles unexpected exit)
    tokio::spawn(async move {
        let _ = child.wait().await;
    });

    Ok(FfmpegResult { pid, stdin })
}

#[tauri::command]
pub async fn start_recording(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    target: Option<CaptureTarget>,
    audio: Option<String>,
) -> Result<String, String> {
    {
        let active = state.recording_active.lock().map_err(|e| e.to_string())?;
        if *active {
            return Err("Recording already in progress".to_string());
        }
    }

    // Dispatch Window captures to the WGC path. All other target types
    // (Screen/Monitor/Region) continue through the existing gdigrab path
    // below.
    let capture_target = target.unwrap_or(CaptureTarget::Screen);
    eprintln!("[recording] start_recording dispatched with target={:?}", capture_target);

    // Persist the live capture target into AppState so stop_recording
    // can thread it through to finalize_recording and the review screen
    // shows the REAL label (e.g. "Notepad") rather than "Entire Screen".
    {
        let mut ct = state
            .current_capture_target
            .lock()
            .map_err(|e| e.to_string())?;
        *ct = Some(crate::commands::recordings::persisted_target_from(
            &capture_target,
        ));
    }

    if matches!(capture_target, CaptureTarget::Window { .. }) {
        eprintln!("[recording] -> routing to WGC window path");
        return start_window_recording(app, state, capture_target, audio).await;
    }
    eprintln!("[recording] -> routing to gdigrab path");

    let ffmpeg_path = resolve_ffmpeg(&app)?;

    let recording_id = uuid::Uuid::new_v4().to_string();
    let output_dir = std::env::temp_dir().join("prospertogether");
    std::fs::create_dir_all(&output_dir).map_err(|e| format!("Cannot create temp dir: {}", e))?;
    let output_path = output_dir.join(format!("recording-{}.webm", recording_id));
    let output_str = output_path.to_string_lossy().to_string();

    let result = spawn_ffmpeg(
        &ffmpeg_path,
        &capture_target,
        audio.as_deref(),
        &output_str,
    )
    .await?;

    // Store process handles (all guards dropped immediately)
    let started_at_ms = now_unix_ms();
    {
        let mut active = state.recording_active.lock().map_err(|e| e.to_string())?;
        *active = true;
    }
    {
        let mut s = state.ffmpeg_stdin.lock().map_err(|e| e.to_string())?;
        *s = result.stdin;
    }
    {
        let mut p = state.ffmpeg_pid.lock().map_err(|e| e.to_string())?;
        *p = result.pid;
    }
    {
        let mut path = state
            .current_recording_path
            .lock()
            .map_err(|e| e.to_string())?;
        *path = Some(output_str.clone());
    }
    {
        let mut s = state
            .recording_started_at_ms
            .lock()
            .map_err(|e| e.to_string())?;
        *s = Some(started_at_ms);
    }

    // Initialize segment tracking
    {
        let mut segments = state.recording_segments.lock().map_err(|e| e.to_string())?;
        segments.clear();
        *state.segment_index.lock().map_err(|e| e.to_string())? = 0;
    }

    // Persist recording state to disk so it survives a Tauri process death.
    if let Some(pid_val) = result.pid {
        let _ = write_state_file(&RecordingStateFile {
            pid: pid_val,
            file_path: output_str.clone(),
            started_at_ms,
        });
    }

    Ok(output_str)
}

/// WGC path for Window captures. Called from `start_recording` when the
/// target is `CaptureTarget::Window`. The gdigrab-path tail of
/// `start_recording` is NOT executed for window captures.
#[cfg(target_os = "windows")]
async fn start_window_recording(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    target: CaptureTarget,
    audio: Option<String>,
) -> Result<String, String> {
    let ffmpeg_path = resolve_ffmpeg(&app)?;

    let recording_id = uuid::Uuid::new_v4().to_string();
    let output_dir = std::env::temp_dir().join("prospertogether");
    std::fs::create_dir_all(&output_dir)
        .map_err(|e| format!("Cannot create temp dir: {}", e))?;
    let output_path = output_dir.join(format!("recording-{}.webm", recording_id));
    let output_str = output_path.to_string_lossy().to_string();

    // Hand the capture thread a clone of the shared rect Arc so it can
    // read the live window position on every frame without going through
    // tauri::State (which isn't accessible from the non-tauri capture
    // thread anyway). `watch_window_rect` — called by the frontend just
    // after start_recording returns — writes the latest rect into this
    // same Arc.
    let rect_slot = state.current_window_rect.clone();

    let handles = crate::commands::window_capture::start_window_capture(
        app.clone(),
        &ffmpeg_path,
        &target,
        audio.as_deref(),
        &output_str,
        rect_slot,
    )
    .await?;

    let started_at_ms = now_unix_ms();

    // Store everything that stop_recording will need. Note: ffmpeg_stdin
    // is NOT populated — the WGC path's writer task owns the stdin handle
    // and closes it when the frame channel drains. stop_recording must
    // check window_capture_stop first and take the gdigrab path only if
    // that is None.
    {
        let mut active = state.recording_active.lock().map_err(|e| e.to_string())?;
        *active = true;
    }
    {
        let mut p = state.ffmpeg_pid.lock().map_err(|e| e.to_string())?;
        *p = handles.ffmpeg_pid;
    }
    {
        let mut path = state
            .current_recording_path
            .lock()
            .map_err(|e| e.to_string())?;
        *path = Some(handles.output_path.clone());
    }
    {
        let mut s = state
            .recording_started_at_ms
            .lock()
            .map_err(|e| e.to_string())?;
        *s = Some(started_at_ms);
    }
    {
        let mut stop = state.window_capture_stop.lock().map_err(|e| e.to_string())?;
        *stop = Some(handles.stop_tx);
    }

    // Initialize segment tracking (matches gdigrab path).
    {
        let mut segments = state.recording_segments.lock().map_err(|e| e.to_string())?;
        segments.clear();
    }
    {
        let mut idx = state.segment_index.lock().map_err(|e| e.to_string())?;
        *idx = 0;
    }

    // Persist recovery state to disk (matches gdigrab path).
    if let Some(pid_val) = handles.ffmpeg_pid {
        let _ = write_state_file(&RecordingStateFile {
            pid: pid_val,
            file_path: handles.output_path.clone(),
            started_at_ms,
        });
    }

    Ok(handles.output_path)
}

#[cfg(not(target_os = "windows"))]
async fn start_window_recording(
    _app: tauri::AppHandle,
    _state: State<'_, AppState>,
    _target: CaptureTarget,
    _audio: Option<String>,
) -> Result<String, String> {
    Err("Window capture is only supported on Windows".to_string())
}

/// Gracefully stop an ffmpeg process: send 'q' to stdin, poll for exit, force
/// kill after timeout. This is shared between `stop_recording` and
/// `switch_capture_target`.
async fn stop_ffmpeg(stdin_opt: Option<ChildStdin>, pid_opt: Option<u32>, timeout_polls: u32) {
    // Send 'q' to FFmpeg stdin for graceful stop, then close stdin
    if let Some(mut stdin) = stdin_opt {
        let _ = stdin.write_all(b"q\n").await;
        let _ = stdin.flush().await;
        drop(stdin);
    }

    // Wait for FFmpeg to exit gracefully (poll every 250ms)
    if let Some(pid) = pid_opt {
        let mut exited = false;
        for _ in 0..timeout_polls {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            let check = Command::new("tasklist")
                .args(["/FI", &format!("PID eq {}", pid), "/NH"])
                .output()
                .await;
            if let Ok(output) = check {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if !stdout.contains(&pid.to_string()) {
                    exited = true;
                    break;
                }
            }
        }
        // Force kill if still running
        if !exited {
            let mut kill_cmd = {
                #[cfg(target_os = "windows")]
                {
                    let mut c = Command::new("taskkill");
                    c.args(["/PID", &pid.to_string(), "/F"]);
                    c
                }
                #[cfg(not(target_os = "windows"))]
                {
                    let mut c = Command::new("kill");
                    c.args(["-9", &pid.to_string()]);
                    c
                }
            };
            apply_no_window(&mut kill_cmd);
            let _ = kill_cmd.output().await;
        }
    }
}

#[tauri::command]
pub async fn stop_recording(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<crate::commands::recordings::RecordingMeta, String> {
    {
        let active = state.recording_active.lock().map_err(|e| e.to_string())?;
        if !*active {
            return Err("No recording in progress".to_string());
        }
    }

    // Compute duration BEFORE we clear `recording_started_at_ms` below.
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

    // Check if this is a WGC window capture. If so, take the stop signal
    // handle and send on it instead of (and in addition to) sending 'q'
    // to stdin — stdin in WGC mode is the frame byte stream, not a
    // command channel.
    let window_stop_tx = {
        let mut stop = state.window_capture_stop.lock().map_err(|e| e.to_string())?;
        stop.take()
    };

    if let Some(stop_tx) = window_stop_tx {
        // WGC path: signal capture thread to stop. The thread will see
        // the flag on its next frame callback and call capture_control.stop().
        // The frame channel then closes, the writer task closes stdin, and
        // ffmpeg exits via the -shortest flag.
        let _ = stop_tx.send(());
        // Small settle time so the capture thread notices and closes stdin
        // before we start polling for ffmpeg exit.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }

    // Extract handles from state BEFORE any await
    let stdin_opt = {
        state.ffmpeg_stdin.lock().map_err(|e| e.to_string())?.take()
    };
    let pid_opt = {
        state.ffmpeg_pid.lock().map_err(|e| e.to_string())?.take()
    };

    // Gracefully stop ffmpeg (poll up to 10s = 40 * 250ms)
    stop_ffmpeg(stdin_opt, pid_opt, 40).await;

    // Handle segment concatenation before clearing state.
    // We collect everything under the lock, then drop the guard before any
    // await (concat_cmd.output()) to keep the future Send.
    let mut recording_path = {
        let path = state
            .current_recording_path
            .lock()
            .map_err(|e| e.to_string())?;
        path.clone().unwrap_or_default()
    };

    let all_segments = collect_and_clear_segments(&state, &recording_path)?;

    if all_segments.len() > 1 {
        // Write concat filelist
        let output_dir = std::env::temp_dir().join("prospertogether");
        let filelist_path = output_dir.join("concat-filelist.txt");
        let filelist_content: String = all_segments
            .iter()
            .map(|s| format!("file '{}'", s.replace('\\', "/")))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&filelist_path, &filelist_content).map_err(|e| e.to_string())?;

        // Generate final output path
        let final_path = output_dir.join(format!("recording-{}.webm", uuid::Uuid::new_v4()));
        let final_str = final_path.to_string_lossy().to_string();

        // Run ffmpeg concat
        let ffmpeg_path = resolve_ffmpeg(&app)?;
        let mut concat_cmd = Command::new(&ffmpeg_path);
        concat_cmd.args(["-f", "concat", "-safe", "0", "-i"]);
        concat_cmd.arg(filelist_path.to_string_lossy().to_string());
        concat_cmd.args(["-c", "copy", "-y"]);
        concat_cmd.arg(&final_str);
        apply_no_window(&mut concat_cmd);

        let output = concat_cmd
            .output()
            .await
            .map_err(|e| format!("Concat failed: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Concat failed: {}", stderr));
        }

        // Cleanup individual segments
        for seg in all_segments.iter() {
            let _ = std::fs::remove_file(seg);
        }
        let _ = std::fs::remove_file(&filelist_path);

        recording_path = final_str;
    }

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

    // Read the persisted capture target from AppState (set by
    // start_recording / start_window_recording) and take ownership so
    // the slot is cleared for the next recording.
    let capture_target = {
        let mut ct = state
            .current_capture_target
            .lock()
            .map_err(|e| e.to_string())?;
        ct.take()
            .unwrap_or(crate::commands::recordings::PersistedCaptureTarget::Screen)
    };

    let meta = crate::commands::recordings::finalize_recording(
        &app,
        &temp_path,
        computed_duration_seconds,
        capture_target,
    )
    .await?;

    Ok(meta)
}

// ─── switch_capture_target ────────────────────────────────────────────────────

#[tauri::command]
pub async fn switch_capture_target(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    target: CaptureTarget,
    audio: Option<String>,
) -> Result<String, String> {
    // 1. Verify recording is active
    {
        let active = state.recording_active.lock().map_err(|e| e.to_string())?;
        if !*active {
            return Err("No recording in progress".to_string());
        }
    }

    // Update persisted capture target in AppState so the final recording
    // saved on stop shows the LAST target the user had active.
    {
        let mut ct = state
            .current_capture_target
            .lock()
            .map_err(|e| e.to_string())?;
        *ct = Some(crate::commands::recordings::persisted_target_from(&target));
    }

    // 2. Extract handles for whichever path is currently running.
    let window_stop_tx = {
        let mut stop = state.window_capture_stop.lock().map_err(|e| e.to_string())?;
        stop.take()
    };
    let stdin_opt = {
        state.ffmpeg_stdin.lock().map_err(|e| e.to_string())?.take()
    };
    let pid_opt = {
        state.ffmpeg_pid.lock().map_err(|e| e.to_string())?.take()
    };
    let current_path = {
        state
            .current_recording_path
            .lock()
            .map_err(|e| e.to_string())?
            .clone()
    };

    // 3-4. Gracefully stop current path. WGC path: send stop signal then
    // poll for ffmpeg exit. gdigrab path: send 'q' to stdin then poll.
    if let Some(stop_tx) = window_stop_tx {
        let _ = stop_tx.send(());
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        stop_ffmpeg(None, pid_opt, 20).await;
    } else {
        stop_ffmpeg(stdin_opt, pid_opt, 20).await;
    }

    // 5. Push completed segment path to recording_segments
    if let Some(ref path) = current_path {
        push_segment(&state, path)?;
    }

    // 6. Increment segment_index
    let seg_idx = next_segment_index(&state)?;

    // 7. Start new ffmpeg with new target
    let ffmpeg_path = resolve_ffmpeg(&app)?;
    let output_dir = std::env::temp_dir().join("prospertogether");
    std::fs::create_dir_all(&output_dir).map_err(|e| format!("Cannot create temp dir: {}", e))?;
    let new_output = output_dir.join(format!(
        "recording-{}-seg{}.webm",
        uuid::Uuid::new_v4(),
        seg_idx
    ));
    let new_output_str = new_output.to_string_lossy().to_string();

    // 7. Start new capture path. Window target -> WGC; others -> gdigrab.
    let (new_pid, new_stdin, new_window_stop) = if matches!(target, CaptureTarget::Window { .. }) {
        #[cfg(target_os = "windows")]
        {
            // Clone the shared rect slot so the capture thread can read
            // live window position on every frame. Watcher is restarted
            // by the frontend right after switch_capture_target returns.
            let rect_slot = state.current_window_rect.clone();
            let handles = crate::commands::window_capture::start_window_capture(
                app.clone(),
                &ffmpeg_path,
                &target,
                audio.as_deref(),
                &new_output_str,
                rect_slot,
            )
            .await?;
            (handles.ffmpeg_pid, None, Some(handles.stop_tx))
        }
        #[cfg(not(target_os = "windows"))]
        {
            return Err("Window capture is only supported on Windows".to_string());
        }
    } else {
        let result = spawn_ffmpeg(
            &ffmpeg_path,
            &target,
            audio.as_deref(),
            &new_output_str,
        )
        .await?;
        (result.pid, result.stdin, None)
    };

    // 8. Update AppState with new handles.
    {
        let mut s = state.ffmpeg_stdin.lock().map_err(|e| e.to_string())?;
        *s = new_stdin;
    }
    {
        let mut p = state.ffmpeg_pid.lock().map_err(|e| e.to_string())?;
        *p = new_pid;
    }
    {
        let mut stop = state.window_capture_stop.lock().map_err(|e| e.to_string())?;
        *stop = new_window_stop;
    }
    {
        let mut path = state
            .current_recording_path
            .lock()
            .map_err(|e| e.to_string())?;
        *path = Some(new_output_str.clone());
    }

    // 9. Update disk state file
    if let Some(pid_val) = new_pid {
        let started_at_ms = state
            .recording_started_at_ms
            .lock()
            .map_err(|e| e.to_string())?
            .unwrap_or_else(now_unix_ms);
        let _ = write_state_file(&RecordingStateFile {
            pid: pid_val,
            file_path: new_output_str.clone(),
            started_at_ms,
        });
    }

    // 10. Return new segment file path
    Ok(new_output_str)
}

// ─── Window watcher ───────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
#[tauri::command]
pub async fn watch_window_rect(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    hwnd: isize,
) -> Result<(), String> {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

    eprintln!("[watch_window_rect] command invoked with hwnd=0x{:X}", hwnd);

    // Abort any existing watcher
    {
        let mut handle = state
            .window_watcher_handle
            .lock()
            .map_err(|e| e.to_string())?;
        if let Some(h) = handle.take() {
            h.abort();
        }
    }

    // Clone the shared rect Arc so the spawned task can update the slot
    // every tick without going through `tauri::State`. The WGC monitor-
    // capture callback holds another clone of this same Arc and reads it
    // on every frame to compute the crop window.
    let rect_arc = state.current_window_rect.clone();

    let app_handle = app.clone();
    let join_handle = tokio::spawn(async move {
        eprintln!("[watch_window_rect] poll loop STARTED for hwnd=0x{:X}", hwnd);
        let mut last_rect: Option<(i32, i32, u32, u32)> = None;
        let mut overlay_was_present = false;
        let mut first_emit_logged = false;

        loop {
            // 16ms ~= 60Hz. GetWindowRect is a non-blocking Win32 call so
            // we run it inline (no spawn_blocking overhead). The whole
            // tick body — Win32 call, Arc lock, conditional set_position
            // — is microseconds when nothing changed.
            tokio::time::sleep(std::time::Duration::from_millis(16)).await;

            let rect_result: Option<(i32, i32, u32, u32)> = unsafe {
                let hwnd_handle = HWND(hwnd as *mut _);
                let mut rect = RECT::default();
                if GetWindowRect(hwnd_handle, &mut rect).is_ok() {
                    let w = (rect.right - rect.left).max(0) as u32;
                    let h = (rect.bottom - rect.top).max(0) as u32;
                    Some((rect.left, rect.top, w, h))
                } else {
                    None
                }
            };

            match rect_result {
                Some((x, y, w, h)) => {
                    let current = (x, y, w, h);
                    let rect_changed = last_rect.as_ref() != Some(&current);

                    // Always refresh the shared slot. The capture callback
                    // reads it on every captured frame, and we want it
                    // current even if the logical rect didn't change
                    // (defensive against the rare case where the slot
                    // was cleared by stop_watching_window).
                    if let Ok(mut slot) = rect_arc.lock() {
                        *slot = Some(current);
                    }

                    // Reposition the overlay only when:
                    //   (a) the target window actually moved, OR
                    //   (b) the overlay JUST appeared (e.g. user re-enabled
                    //       drawing after moving the window — the new
                    //       overlay would be at a stale captureTarget.rect
                    //       and needs to snap to the current position).
                    // Both are infrequent events; idle ticks are basically
                    // free (one Win32 call + one mutex lock).
                    let overlay_now = app_handle.get_webview_window("overlay");
                    let overlay_present = overlay_now.is_some();
                    let overlay_just_appeared = overlay_present && !overlay_was_present;
                    overlay_was_present = overlay_present;

                    if let Some(overlay) = overlay_now {
                        if rect_changed || overlay_just_appeared {
                            let pos = tauri::PhysicalPosition::new(x, y);
                            let size = tauri::PhysicalSize::new(w, h);
                            let _ = overlay.set_position(pos);
                            let _ = overlay.set_size(size);

                            static FIRST_SYNC_LOGGED: std::sync::atomic::AtomicBool =
                                std::sync::atomic::AtomicBool::new(false);
                            if !FIRST_SYNC_LOGGED
                                .swap(true, std::sync::atomic::Ordering::Relaxed)
                            {
                                eprintln!(
                                    "[watch_window_rect] FIRST overlay sync at ({}, {}) {}x{} (overlay_just_appeared={})",
                                    x, y, w, h, overlay_just_appeared
                                );
                            }
                        }
                    }

                    if rect_changed {
                        last_rect = Some(current);

                        if !first_emit_logged {
                            first_emit_logged = true;
                            eprintln!(
                                "[watch_window_rect] FIRST move: ({}, {}) {}x{}",
                                x, y, w, h
                            );
                        }

                        // Still fire the JS event so the BorderPage in
                        // region/monitor mode can reposition itself.
                        let _ = app_handle.emit(
                            "border:reposition",
                            serde_json::json!({
                                "x": x,
                                "y": y,
                                "width": w,
                                "height": h,
                            }),
                        );
                    }
                }
                None => {
                    // GetWindowRect failed → window closed.
                    if let Ok(mut slot) = rect_arc.lock() {
                        *slot = None;
                    }
                    let _ = app_handle.emit("border:target-closed", ());
                    break;
                }
            }
        }
    });

    {
        let mut handle = state
            .window_watcher_handle
            .lock()
            .map_err(|e| e.to_string())?;
        *handle = Some(join_handle);
    }

    Ok(())
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
pub async fn watch_window_rect(
    _app: tauri::AppHandle,
    _state: State<'_, AppState>,
    _hwnd: isize,
) -> Result<(), String> {
    Ok(())
}

#[tauri::command]
pub async fn stop_watching_window(state: State<'_, AppState>) -> Result<(), String> {
    let mut handle = state
        .window_watcher_handle
        .lock()
        .map_err(|e| e.to_string())?;
    if let Some(h) = handle.take() {
        h.abort();
    }
    // Clear the shared rect slot so any still-running capture callback
    // stops trying to crop against stale coordinates. The capture thread
    // handles `None` by writing a fully black frame at the initial crop
    // dimensions, which is correct behavior for a closed/stopped target.
    if let Ok(mut slot) = state.current_window_rect.lock() {
        *slot = None;
    }
    Ok(())
}

/// Diagnostic command: print a message to Rust stderr. Used from frontend
/// listener code so we can verify event flow from the terminal without
/// needing to open a specific webview's devtools. Safe to leave in place —
/// logging is cheap.
#[tauri::command]
pub fn debug_log(message: String) {
    eprintln!("[debug_log] {}", message);
}

/// Inspect the in-memory recording state. The frontend calls this on mount
/// to detect a recovered or in-flight recording and jump straight back into
/// the dock UI rather than showing the idle "Ready to Record" screen.
#[tauri::command]
pub async fn get_recording_state(
    state: State<'_, AppState>,
) -> Result<RecordingStateDto, String> {
    let active = *state.recording_active.lock().map_err(|e| e.to_string())?;
    let file_path = state
        .current_recording_path
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    let started_at_ms = *state
        .recording_started_at_ms
        .lock()
        .map_err(|e| e.to_string())?;

    let duration_seconds = match started_at_ms {
        Some(ms) => {
            let now = now_unix_ms();
            if now >= ms {
                (now - ms) / 1000
            } else {
                0
            }
        }
        None => 0,
    };

    Ok(RecordingStateDto {
        active,
        file_path,
        started_at_ms,
        duration_seconds,
    })
}
