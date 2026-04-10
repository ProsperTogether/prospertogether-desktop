use std::sync::{Arc, Mutex};

use tokio::process::ChildStdin;

#[derive(Default)]
pub struct AppState {
    pub recording_active: Mutex<bool>,
    pub upload_active: Mutex<bool>,
    pub transcription_active: Mutex<bool>,
    pub ffmpeg_stdin: Mutex<Option<ChildStdin>>,
    pub ffmpeg_pid: Mutex<Option<u32>>,
    pub current_recording_path: Mutex<Option<String>>,
    /// Unix milliseconds when the current recording was started.
    /// Set on start_recording success, cleared on stop_recording.
    /// Used to compute elapsed duration after a recovery from a previous
    /// process death (when the in-memory duration counter is gone).
    pub recording_started_at_ms: Mutex<Option<u64>>,
    pub window_watcher_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
    pub recording_segments: Mutex<Vec<String>>,
    pub segment_index: Mutex<u32>,
    /// Control handle for an active Windows Graphics Capture session.
    /// Sending on this oneshot stops the capture thread, which closes the
    /// frame channel, which causes ffmpeg to see EOF on its rawvideo stdin
    /// input and finalize the output file. `None` when window capture is
    /// not active (i.e., when idle or when recording via gdigrab path).
    pub window_capture_stop: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    /// The capture target for the currently-running recording. Preserved
    /// across the stop flow so `finalize_recording` can persist a real
    /// label into metadata.json (window title, monitor size, etc.) rather
    /// than a generic "Entire Screen" placeholder.
    pub current_capture_target: Mutex<Option<crate::commands::recordings::PersistedCaptureTarget>>,
    /// Shared rect of the window being recorded, in physical pixels on
    /// the virtual desktop. Updated every ~500ms by `watch_window_rect`.
    /// Read by the WGC monitor-capture callback to crop each monitor
    /// frame to the current window area so the recording follows the
    /// window as the user moves it. The tuple is `(x, y, width, height)`.
    ///
    /// Wrapped in `Arc<Mutex<...>>` (rather than a plain `Mutex<...>`)
    /// so the capture thread and the watcher task can each hold a clone
    /// of the Arc without going through a tauri::State lookup on every
    /// frame (the capture thread is not inside a tauri command context).
    pub current_window_rect: Arc<Mutex<Option<(i32, i32, u32, u32)>>>,
}
