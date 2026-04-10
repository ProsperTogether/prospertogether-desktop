# Window-Follow Capture Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the rect-based gdigrab window capture with Windows Graphics Capture (WGC) so recordings follow the target window as it moves around the screen.

**Architecture:** Add a new `window_capture` Rust module that spawns a WGC session on its own thread, throttles frames to 15fps, and pipes raw BGRA bytes to ffmpeg via stdin. `start_recording` dispatches to the WGC path for `CaptureTarget::Window` and keeps the existing gdigrab path for Screen/Monitor/Region modes. `stop_recording` and `switch_capture_target` handle both paths uniformly via the existing segment-management flow.

**Tech Stack:** Rust (Tauri 2 backend), `windows-capture` crate v1.4, ffmpeg (rawvideo input), existing tokio async runtime.

**Working directory:** `C:/Code/happier/portal/agent/`

**Not a git repository** — per-task verification is `cargo check` (not `git commit`). Do NOT attempt to run `git` commands.

---

## File Structure

### Created
- `src-tauri/src/commands/window_capture.rs` — WGC session management, frame channel, ffmpeg spawning for raw video input, stop logic. Primary new code file.

### Modified
- `src-tauri/Cargo.toml` — add `windows-capture` dependency
- `src-tauri/src/state.rs` — add `window_capture_stop` field to `AppState`
- `src-tauri/src/commands/recording.rs` — `start_recording` dispatches by target type; remove rect-based Window branch from `spawn_ffmpeg`; update `stop_recording` and `switch_capture_target` to handle both paths
- `src-tauri/src/commands/mod.rs` — export new module
- `src-tauri/src/lib.rs` — no command changes (dispatching is internal); any new imports only

### Untouched (explicit)
- All frontend code — the TS layer already passes `CaptureTarget::Window { hwnd, rect, title }` and doesn't care how capture works internally
- Border overlay + window rect watcher — still used for visual feedback
- `list_windows` command — the HWND it returns is the same one we bind WGC to

---

## Task 1: Add windows-capture dependency

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: Add the crate to Windows dependencies**

Find the `[target.'cfg(target_os = "windows")'.dependencies]` section (currently contains `webview2-com`, `windows`, `image`). Add:

```toml
windows-capture = "1.4"
```

The final section should look like:
```toml
[target.'cfg(target_os = "windows")'.dependencies]
webview2-com = "0.38"
windows = { version = "0.58", features = [
    "Win32_Foundation",
    "Win32_UI_WindowsAndMessaging",
    "Win32_Graphics_Gdi",
    "Win32_System_Threading",
    "Win32_Graphics_Dwm",
] }
image = { version = "0.25", default-features = false, features = ["png"] }
windows-capture = "1.4"
```

- [ ] **Step 2: Verify the crate resolves**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`

Expected: compiles cleanly (existing code unchanged). The new crate is downloaded but not yet imported.

If `windows-capture` v1.4 is not available, try v1.3 or check the latest on crates.io. The API used in this plan is the stable `GraphicsCaptureApiHandler` trait pattern which has been consistent since v1.0.

---

## Task 2: Add AppState field for window capture stop signal

**Files:**
- Modify: `src-tauri/src/state.rs`

- [ ] **Step 1: Add the field**

Open `src-tauri/src/state.rs`. The current file is:

```rust
use std::sync::Mutex;

use tokio::process::ChildStdin;

#[derive(Default)]
pub struct AppState {
    pub recording_active: Mutex<bool>,
    pub upload_active: Mutex<bool>,
    pub transcription_active: Mutex<bool>,
    pub ffmpeg_stdin: Mutex<Option<ChildStdin>>,
    pub ffmpeg_pid: Mutex<Option<u32>>,
    pub current_recording_path: Mutex<Option<String>>,
    pub recording_started_at_ms: Mutex<Option<u64>>,
    pub window_watcher_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
    pub recording_segments: Mutex<Vec<String>>,
    pub segment_index: Mutex<u32>,
}
```

Add a new field after `segment_index`:

```rust
    /// Control handle for an active Windows Graphics Capture session.
    /// Sending on this oneshot stops the capture thread, which closes the
    /// frame channel, which causes ffmpeg to see EOF on its rawvideo stdin
    /// input and finalize the output file. `None` when window capture is
    /// not active (i.e., when idle or when recording via gdigrab path).
    pub window_capture_stop: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
```

- [ ] **Step 2: Verify compile**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`

Expected: compiles cleanly. The new field adds a `dead_code` warning until it's read elsewhere, which is fine.

---

## Task 3: Create window_capture module skeleton

**Files:**
- Create: `src-tauri/src/commands/window_capture.rs`
- Modify: `src-tauri/src/commands/mod.rs`

- [ ] **Step 1: Create the module file with skeleton**

Create `src-tauri/src/commands/window_capture.rs` with:

```rust
//! Windows Graphics Capture-based window recording.
//!
//! This module implements per-window screen capture using the WGC WinRT API
//! (via the `windows-capture` crate). Unlike gdigrab's rect-based capture,
//! WGC follows the target window through moves, monitor changes, and
//! occlusion — the user can reposition the window during recording without
//! the capture area becoming stale.
//!
//! ## Architecture
//!
//! ```text
//! WGC session (own thread)    Frame channel        ffmpeg child
//!        │                         │                    │
//!        │ on_frame_arrived        │                    │
//!        ├─ throttle to 15fps  ──► │ (bounded)          │
//!        │                         │                    │
//!        │                         ├──────── async ────►│ stdin
//!        │                         │    writer task     │ (rawvideo)
//!        │                         │                    │
//!        │ on_closed                                    ▼
//!        └─► emit border:target-closed          output.webm
//! ```
//!
//! The WGC session and the async writer task are joined via a bounded
//! `tokio::sync::mpsc` channel that carries raw BGRA frame buffers.
//! Backpressure: if ffmpeg falls behind encoding, frames are dropped
//! via `try_send` rather than allowed to grow unboundedly.

#![cfg(target_os = "windows")]

use std::path::Path;
use std::process::Stdio;

use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;
use tokio::process::{ChildStdin, Command};
use tokio::sync::{mpsc, oneshot};

use crate::capture_target::CaptureTarget;

/// One captured frame: raw BGRA bytes in row-major order, top-down.
struct Frame {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

/// Result of starting a window capture session. The caller stores these
/// handles in AppState so `stop_recording` can signal the capture thread
/// and wait for the ffmpeg child to exit.
pub struct WindowCaptureHandles {
    pub stop_tx: oneshot::Sender<()>,
    pub ffmpeg_pid: Option<u32>,
    pub output_path: String,
}

/// Public entry point. Starts a WGC capture session for the given HWND
/// and an ffmpeg child that reads raw BGRA frames from stdin. Returns the
/// handles needed to stop the session and the output file path.
///
/// Call from within `start_recording` when the target is `CaptureTarget::Window`.
pub async fn start_window_capture(
    _app: AppHandle,
    _ffmpeg_path: &Path,
    _target: &CaptureTarget,
    _audio: Option<&str>,
    _output_path: &str,
) -> Result<WindowCaptureHandles, String> {
    // Filled in by later tasks
    Err("not implemented".to_string())
}
```

- [ ] **Step 2: Register the module**

Open `src-tauri/src/commands/mod.rs` (currently contains `pub mod auth;` etc) and add:

```rust
pub mod window_capture;
```

The updated file:
```rust
pub mod auth;
pub mod devices;
pub mod frames;
pub mod recording;
pub mod setup;
pub mod transcription;
pub mod updater;
pub mod upload;
pub mod window_capture;
```

- [ ] **Step 3: Verify compile**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`

Expected: compiles cleanly. Warnings about unused `Frame`, `WindowCaptureHandles`, `start_window_capture` are fine — they will be used in later tasks.

---

## Task 4: Build ffmpeg command for rawvideo input

**Files:**
- Modify: `src-tauri/src/commands/window_capture.rs`

- [ ] **Step 1: Add the ffmpeg arg builder function**

Add this function at the end of `src-tauri/src/commands/window_capture.rs`:

```rust
/// Build the ffmpeg argument list for a raw BGRA video input from stdin
/// plus an optional dshow audio input. Mirrors the encoding parameters of
/// the existing gdigrab path (VP8 + Opus at 1 Mbps, 15fps, scaled to max
/// 1920 width) so output files from both paths are compatible with the
/// segment concat flow.
fn build_ffmpeg_args(
    width: u32,
    height: u32,
    audio: Option<&str>,
    output_path: &str,
) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();

    // Audio input FIRST (dshow must come before video input for proper sync,
    // matching the existing gdigrab path's ordering).
    let audio_device = audio.unwrap_or("");
    if !audio_device.is_empty() && audio_device != "none" {
        args.extend([
            "-f".into(),
            "dshow".into(),
            "-i".into(),
            format!("audio={}", audio_device),
        ]);
    }

    // Raw BGRA video from stdin. Size is fixed at capture-session start; a
    // window resize during recording triggers a segment restart (handled
    // elsewhere).
    args.extend([
        "-f".into(),
        "rawvideo".into(),
        "-pix_fmt".into(),
        "bgra".into(),
        "-s".into(),
        format!("{}x{}", width, height),
        "-framerate".into(),
        "15".into(),
        "-i".into(),
        "pipe:0".into(),
    ]);

    // Video encoding: scale down (don't upscale small windows), convert to
    // yuv420p for VP8, 1 Mbps realtime encode.
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

    // -shortest ensures ffmpeg stops when the shorter of audio/video ends.
    // Without this, if audio has no natural EOF, ffmpeg would wait forever
    // after we close the video stdin pipe.
    args.extend(["-shortest".into(), "-y".into(), output_path.to_string()]);

    args
}
```

- [ ] **Step 2: Verify compile**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`

Expected: compiles cleanly. Warning about `build_ffmpeg_args` being unused is fine.

---

## Task 5: Spawn ffmpeg child process with piped stdin

**Files:**
- Modify: `src-tauri/src/commands/window_capture.rs`

- [ ] **Step 1: Add the ffmpeg spawner function**

Add at the end of `src-tauri/src/commands/window_capture.rs`:

```rust
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

fn apply_no_window(cmd: &mut Command) {
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
}

/// Spawn ffmpeg with rawvideo input on stdin, matching the existing
/// recording.rs pattern: stdin=piped, stdout=null, stderr=logfile. Returns
/// the child process's pid and its piped stdin handle. Also spawns a
/// background task that waits for the child to exit (matches existing
/// gdigrab path behavior — prevents zombie processes).
async fn spawn_ffmpeg_child(
    ffmpeg_path: &Path,
    width: u32,
    height: u32,
    audio: Option<&str>,
    output_path: &str,
) -> Result<(Option<u32>, ChildStdin), String> {
    let args = build_ffmpeg_args(width, height, audio, output_path);

    // stderr log path — same pattern as recording.rs
    let output_dir = std::path::Path::new(output_path)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::temp_dir().join("userfirst"));
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
        .map_err(|e| format!("Failed to start ffmpeg (rawvideo): {}", e))?;

    let pid = child.id();
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "ffmpeg stdin not captured".to_string())?;

    // Startup crash check (same as gdigrab path).
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
        let log = std::fs::read_to_string(&log_path).unwrap_or_default();
        let last: String = log
            .lines()
            .rev()
            .take(5)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        return Err(format!(
            "ffmpeg (rawvideo) exited immediately (code {:?}):\n{}",
            status.code(),
            last
        ));
    }

    // Background waiter to prevent zombie.
    tokio::spawn(async move {
        let _ = child.wait().await;
    });

    Ok((pid, stdin))
}
```

- [ ] **Step 2: Verify compile**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`

Expected: compiles cleanly. `dead_code` warnings on unused helpers are fine.

---

## Task 6: Frame writer task (pipes frames from channel to ffmpeg stdin)

**Files:**
- Modify: `src-tauri/src/commands/window_capture.rs`

- [ ] **Step 1: Add the writer task function**

Add at the end of `src-tauri/src/commands/window_capture.rs`:

```rust
/// Background task that drains the frame channel and writes raw BGRA bytes
/// to ffmpeg's stdin. Exits when the channel is closed (capture session
/// stopped) or when stdin write fails (ffmpeg died). Closing the stdin
/// handle on task exit lets ffmpeg finalize the output file cleanly via
/// the -shortest flag.
async fn run_frame_writer(mut rx: mpsc::Receiver<Frame>, mut stdin: ChildStdin) {
    while let Some(frame) = rx.recv().await {
        if let Err(e) = stdin.write_all(&frame.data).await {
            eprintln!("[window_capture] ffmpeg stdin write failed: {}", e);
            break;
        }
    }
    // Explicit flush + drop ensures ffmpeg sees EOF immediately rather
    // than lingering until the handle is eventually garbage-collected.
    let _ = stdin.flush().await;
    let _ = stdin.shutdown().await;
    drop(stdin);
}
```

- [ ] **Step 2: Verify compile**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`

Expected: compiles cleanly.

---

## Task 7: WGC capture handler struct and frame throttling

**Files:**
- Modify: `src-tauri/src/commands/window_capture.rs`

Before writing code, read the `windows-capture` crate's actual v1.4 API via its docs to confirm trait names, frame buffer access methods, and `start` signature. If the exact API differs from what's shown below, adapt while keeping the same structure.

- [ ] **Step 1: Add capture handler struct**

Add at the end of `src-tauri/src/commands/window_capture.rs`:

```rust
use std::time::{Duration, Instant};

use windows_capture::{
    capture::{Context, GraphicsCaptureApiHandler},
    frame::Frame as WgcFrame,
    graphics_capture_api::InternalCaptureControl,
    settings::{ColorFormat, CursorCaptureSettings, DrawBorderSettings, Settings},
    window::Window as WgcWindow,
};

/// Data passed from `start_window_capture` into the `new()` constructor of
/// `WindowCaptureHandler` when the capture session spawns. The crate's API
/// takes a single typed `Flags` value to seed the handler.
struct HandlerFlags {
    frame_tx: mpsc::Sender<Frame>,
    app: AppHandle,
    /// Notified when the session reports a resize; carries the new size.
    resize_tx: mpsc::UnboundedSender<(u32, u32)>,
}

struct WindowCaptureHandler {
    frame_tx: mpsc::Sender<Frame>,
    app: AppHandle,
    resize_tx: mpsc::UnboundedSender<(u32, u32)>,
    last_frame_at: Option<Instant>,
    current_width: u32,
    current_height: u32,
}

const TARGET_FPS: u64 = 15;
const MIN_FRAME_INTERVAL: Duration = Duration::from_millis(1000 / TARGET_FPS);

impl GraphicsCaptureApiHandler for WindowCaptureHandler {
    type Flags = HandlerFlags;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        let flags = ctx.flags;
        Ok(Self {
            frame_tx: flags.frame_tx,
            app: flags.app,
            resize_tx: flags.resize_tx,
            last_frame_at: None,
            current_width: 0,
            current_height: 0,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut WgcFrame,
        _capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        // Throttle: drop frames that arrive too fast for our 15fps target.
        let now = Instant::now();
        if let Some(prev) = self.last_frame_at {
            if now.duration_since(prev) < MIN_FRAME_INTERVAL {
                return Ok(());
            }
        }
        self.last_frame_at = Some(now);

        // Extract width/height and check for resize.
        let width = frame.width();
        let height = frame.height();
        if self.current_width == 0 {
            // First frame — record the baseline.
            self.current_width = width;
            self.current_height = height;
        } else if width != self.current_width || height != self.current_height {
            // Size changed. Signal the outer task to restart ffmpeg with the
            // new dimensions. Don't push this frame — it belongs to the new
            // segment, which doesn't exist yet.
            let _ = self.resize_tx.send((width, height));
            self.current_width = width;
            self.current_height = height;
            return Ok(());
        }

        // Copy frame bytes into an owned buffer for the channel.
        let mut buffer = frame.buffer()?;
        let raw = buffer.as_raw_buffer();
        let data = raw.to_vec();

        let owned = Frame {
            width,
            height,
            data,
        };

        // Non-blocking send with drop-on-full semantics (backpressure).
        if self.frame_tx.try_send(owned).is_err() {
            // Encoder is behind. Drop this frame rather than blocking the
            // capture thread — a dropped frame at 15fps is barely visible,
            // but a blocked capture thread would stall the WGC pipeline.
        }

        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        // Window was closed. Emit the same event the gdigrab-path window
        // watcher uses so the frontend's existing handler triggers stop.
        let _ = self.app.emit("border:target-closed", ());
        Ok(())
    }
}
```

- [ ] **Step 2: Verify compile**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`

Expected: compiles cleanly. If the `windows-capture` crate's API has slightly different names (e.g., `Context` might be passed differently, `width()`/`height()` might be `Frame::width` fields, `buffer()` might return a different type), adapt based on the crate docs — the structure stays the same.

**If compile fails due to API mismatch**, report BLOCKED with the specific compiler error and the crate API you observed. Do not guess — read the crate source or docs first.

---

## Task 8: Implement start_window_capture

**Files:**
- Modify: `src-tauri/src/commands/window_capture.rs`

- [ ] **Step 1: Replace the stub with the real implementation**

Replace the existing `start_window_capture` stub in `src-tauri/src/commands/window_capture.rs` with:

```rust
pub async fn start_window_capture(
    app: AppHandle,
    ffmpeg_path: &Path,
    target: &CaptureTarget,
    audio: Option<&str>,
    output_path: &str,
) -> Result<WindowCaptureHandles, String> {
    // Extract HWND and initial size from the target.
    let (hwnd, initial_w, initial_h) = match target {
        CaptureTarget::Window { hwnd, rect, .. } => (*hwnd, rect.width, rect.height),
        _ => return Err("start_window_capture called with non-Window target".to_string()),
    };

    // Bounded frame channel with capacity for 3 frames (backpressure).
    let (frame_tx, frame_rx) = mpsc::channel::<Frame>(3);
    // Resize signal channel (rare, unbounded is fine).
    let (resize_tx, _resize_rx) = mpsc::unbounded_channel::<(u32, u32)>();
    // Stop signal: caller sends on this to stop the capture thread.
    let (stop_tx, stop_rx) = oneshot::channel::<()>();

    // Spawn ffmpeg first — if it fails, we don't want a capture session
    // running with nowhere to send frames.
    let (ffmpeg_pid, stdin) =
        spawn_ffmpeg_child(ffmpeg_path, initial_w, initial_h, audio, output_path).await?;

    // Spawn the writer task. It exits when the frame channel closes.
    tokio::spawn(run_frame_writer(frame_rx, stdin));

    // Look up the WGC window handle from the HWND.
    let wgc_window = WgcWindow::from_raw_hwnd(hwnd as *mut _);
    // Verify the HWND is still valid.
    if wgc_window.is_err() {
        return Err(format!("HWND {} is not a valid window handle", hwnd));
    }
    let wgc_window = wgc_window.map_err(|e| format!("WGC window lookup failed: {:?}", e))?;

    // Settings: capture cursor, no border overlay (we draw our own), BGRA8.
    let settings = Settings::new(
        wgc_window,
        CursorCaptureSettings::WithCursor,
        DrawBorderSettings::WithoutBorder,
        ColorFormat::Bgra8,
        HandlerFlags {
            frame_tx,
            app: app.clone(),
            resize_tx,
        },
    );

    // Spawn the capture session on its own OS thread. The `start` method
    // blocks until the session ends (either via on_closed or via returning
    // an error from on_frame_arrived), so it must not run on the tokio
    // runtime or it would block an executor thread indefinitely.
    std::thread::spawn(move || {
        if let Err(e) = WindowCaptureHandler::start(settings) {
            eprintln!("[window_capture] capture session error: {:?}", e);
        }
    });

    // Bridge the stop_rx oneshot to the capture session: when the caller
    // stops, we drop the frame_tx clone held by the handler, which closes
    // the channel, which exits the writer task, which closes stdin, which
    // lets ffmpeg finalize the file.
    //
    // Note: windows-capture provides `InternalCaptureControl::stop()` inside
    // the frame callback for mid-capture stop, but from outside the thread
    // we can't access it directly. Instead we rely on: (a) on_closed firing
    // when the window is destroyed, or (b) dropping our channel senders so
    // the handler's try_send fails and we simply let the thread continue
    // running until the window closes.
    //
    // For explicit stop, we need another mechanism. The simplest: hold a
    // shared atomic flag that the handler checks each frame; if set, it
    // calls capture_control.stop(). See Task 9.
    let _ = stop_rx; // placeholder until Task 9

    Ok(WindowCaptureHandles {
        stop_tx,
        ffmpeg_pid,
        output_path: output_path.to_string(),
    })
}
```

- [ ] **Step 2: Verify compile**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`

Expected: compiles cleanly. Warning about `_resize_rx` being unused is expected — Task 10 wires it up.

---

## Task 9: Wire the stop signal into the capture handler

**Files:**
- Modify: `src-tauri/src/commands/window_capture.rs`

The problem: the `start_window_capture` function creates a `stop_tx: oneshot::Sender<()>` and the caller stores it. But the running capture session doesn't see it — it has no way to know when to stop. We need to bridge the two.

Approach: use an `Arc<AtomicBool>` that both the outer code and the handler share. The outer `stop_rx` future sets the flag when triggered; the handler checks it each frame and calls `capture_control.stop()` if set.

- [ ] **Step 1: Add the shared stop flag to HandlerFlags and handler**

In `src-tauri/src/commands/window_capture.rs`, update `HandlerFlags` and `WindowCaptureHandler`:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

struct HandlerFlags {
    frame_tx: mpsc::Sender<Frame>,
    app: AppHandle,
    resize_tx: mpsc::UnboundedSender<(u32, u32)>,
    stop_flag: Arc<AtomicBool>,
}

struct WindowCaptureHandler {
    frame_tx: mpsc::Sender<Frame>,
    app: AppHandle,
    resize_tx: mpsc::UnboundedSender<(u32, u32)>,
    stop_flag: Arc<AtomicBool>,
    last_frame_at: Option<Instant>,
    current_width: u32,
    current_height: u32,
}
```

Update the `new()` impl to populate `stop_flag`:
```rust
    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        let flags = ctx.flags;
        Ok(Self {
            frame_tx: flags.frame_tx,
            app: flags.app,
            resize_tx: flags.resize_tx,
            stop_flag: flags.stop_flag,
            last_frame_at: None,
            current_width: 0,
            current_height: 0,
        })
    }
```

Update `on_frame_arrived` to check the flag at the very top:
```rust
    fn on_frame_arrived(
        &mut self,
        frame: &mut WgcFrame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        // Honor the external stop signal.
        if self.stop_flag.load(Ordering::Relaxed) {
            capture_control.stop();
            return Ok(());
        }

        // ... rest of existing body unchanged
    }
```

Note: the `capture_control` parameter name had a leading underscore in Task 7 (`_capture_control`). Remove the underscore since we now use it.

- [ ] **Step 2: Wire stop_rx to set the flag**

In `start_window_capture`, replace the placeholder `let _ = stop_rx;` line and the section below it with:

```rust
    // Shared stop flag bridges the outer oneshot to the handler's frame
    // callback. When the caller sends on stop_tx (stored in AppState),
    // the spawn_on_stop task flips this flag; the handler sees it on the
    // next frame and calls capture_control.stop() to end the session.
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_for_bridge = stop_flag.clone();
    tokio::spawn(async move {
        let _ = stop_rx.await;
        stop_flag_for_bridge.store(true, Ordering::Relaxed);
    });

    // Settings: capture cursor, no border overlay (we draw our own), BGRA8.
    let settings = Settings::new(
        wgc_window,
        CursorCaptureSettings::WithCursor,
        DrawBorderSettings::WithoutBorder,
        ColorFormat::Bgra8,
        HandlerFlags {
            frame_tx,
            app: app.clone(),
            resize_tx,
            stop_flag,
        },
    );

    // Spawn the capture session on its own OS thread.
    std::thread::spawn(move || {
        if let Err(e) = WindowCaptureHandler::start(settings) {
            eprintln!("[window_capture] capture session error: {:?}", e);
        }
    });

    Ok(WindowCaptureHandles {
        stop_tx,
        ffmpeg_pid,
        output_path: output_path.to_string(),
    })
}
```

- [ ] **Step 3: Verify compile**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`

Expected: compiles cleanly.

---

## Task 10: Defer resize handling to a later pass

**Files:**
- Modify: `src-tauri/src/commands/window_capture.rs`

Full resize handling requires stopping and restarting ffmpeg with a new `-s WxH` mid-recording, which duplicates the segment-restart logic already in `switch_capture_target`. To keep this plan tractable, **ship the initial implementation without automatic resize handling**. When a resize occurs, the capture simply continues at the original dimensions (the larger/smaller window frames will be silently stretched or cropped by ffmpeg's scale filter).

This is an acceptable v1: users rarely resize windows mid-recording (moves are far more common), and moves are fully handled by WGC. Resize can be added as a follow-up.

- [ ] **Step 1: Make the resize channel a no-op**

In `src-tauri/src/commands/window_capture.rs`, in `on_frame_arrived`, update the resize branch to log and continue rather than signalling:

```rust
        } else if width != self.current_width || height != self.current_height {
            // Window was resized. v1 limitation: we continue capture at the
            // original dimensions. The new frames will be stretched/cropped
            // by ffmpeg's scale filter on encode. Log once for visibility.
            eprintln!(
                "[window_capture] window resized from {}x{} to {}x{} — \
                continuing at original size (v1 limitation)",
                self.current_width, self.current_height, width, height
            );
            // Reset current_* so we don't spam the log on every subsequent frame.
            self.current_width = width;
            self.current_height = height;
            return Ok(());
        }
```

Also remove the `resize_tx` field from `HandlerFlags` and `WindowCaptureHandler` since nothing consumes the signal. Remove the `resize_tx: mpsc::UnboundedSender<(u32, u32)>` field from both structs, remove the population in `new()`, and remove the `let (resize_tx, _resize_rx) = ...` line + the `resize_tx` in the `HandlerFlags` struct literal in `start_window_capture`.

**Important:** re-read the file after deleting these lines and make sure no references to `resize_tx` remain. The compiler will catch missed ones.

- [ ] **Step 2: Verify compile**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`

Expected: compiles cleanly with no warnings about unused `resize_tx`.

---

## Task 11: Update state.rs tracking — mark path type

**Files:**
- Modify: `src-tauri/src/state.rs`

The `stop_recording` and `switch_capture_target` functions need to know which capture path is active (gdigrab or WGC) so they can stop it correctly. Using the presence of `window_capture_stop` as the signal works but is fragile. Make it explicit.

Actually, the presence-based signal is fine AND simpler: if `window_capture_stop` is `Some`, we're in WGC mode; if `None`, we're in gdigrab mode (or idle). Stick with that. No change to state.rs needed beyond Task 2.

- [ ] **Step 1: Verify no changes needed**

Confirm by reading the current `state.rs` that it already has the `window_capture_stop` field from Task 2. If yes, mark this task complete with no code changes.

---

## Task 12: Dispatch in start_recording

**Files:**
- Modify: `src-tauri/src/commands/recording.rs`

- [ ] **Step 1: Read the current start_recording function**

Read `src-tauri/src/commands/recording.rs` lines 340-460 (approximate — the function starts at the `#[tauri::command] pub async fn start_recording` attribute and ends at its closing brace). Note the structure: it currently calls `spawn_ffmpeg` for all target types and stores `ffmpeg_stdin`, `ffmpeg_pid`, `current_recording_path`, etc. in AppState.

- [ ] **Step 2: Add a dispatch branch at the start of the function**

Inside `start_recording`, immediately after the early-return check for `recording_active`, insert a dispatch branch for the Window target:

```rust
    // Dispatch Window captures to the WGC path. All other target types
    // (Screen/Monitor/Region) continue through the existing gdigrab path
    // below.
    let target_unwrapped = target.clone().unwrap_or(CaptureTarget::Screen);
    if matches!(target_unwrapped, CaptureTarget::Window { .. }) {
        return start_window_recording(app, state, target_unwrapped, audio).await;
    }
```

Where `start_window_recording` is a new function defined later in `recording.rs` (Task 13 fills it in).

The rest of `start_recording` below this dispatch continues unchanged (builds ffmpeg args for the gdigrab path).

- [ ] **Step 3: Verify compile**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`

Expected: fails with "cannot find function `start_window_recording` in this scope". Good — Task 13 adds it.

---

## Task 13: Add start_window_recording helper in recording.rs

**Files:**
- Modify: `src-tauri/src/commands/recording.rs`

- [ ] **Step 1: Add the function**

Add `start_window_recording` as a new private async function in `src-tauri/src/commands/recording.rs`. Place it right after `start_recording` (before `stop_recording`). It mirrors the storage side of `start_recording` but calls into `window_capture::start_window_capture`:

```rust
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
    let output_dir = std::env::temp_dir().join("userfirst");
    std::fs::create_dir_all(&output_dir)
        .map_err(|e| format!("Cannot create temp dir: {}", e))?;
    let output_path = output_dir.join(format!("recording-{}.webm", recording_id));
    let output_str = output_path.to_string_lossy().to_string();

    let handles = crate::commands::window_capture::start_window_capture(
        app.clone(),
        &ffmpeg_path,
        &target,
        audio.as_deref(),
        &output_str,
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
```

- [ ] **Step 2: Verify compile**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`

Expected: compiles cleanly (both dispatch and implementation now present).

---

## Task 14: Dispatch stop_recording by active path

**Files:**
- Modify: `src-tauri/src/commands/recording.rs`

- [ ] **Step 1: Read the current stop_recording function**

Read `src-tauri/src/commands/recording.rs` and find `stop_recording`. Note that it extracts `ffmpeg_stdin`, sends 'q', and waits for the process. For the WGC path, we instead take the `window_capture_stop` oneshot sender and send `()` on it.

- [ ] **Step 2: Add dispatch at the top of stop_recording**

Immediately after the early-return check for `recording_active`, add:

```rust
    // Check if this is a WGC window capture. If so, use the stop signal
    // path instead of sending 'q' to stdin (which in WGC mode is the
    // frame byte stream, not a command channel).
    let window_stop_tx = {
        let mut stop = state.window_capture_stop.lock().map_err(|e| e.to_string())?;
        stop.take()
    };

    if let Some(stop_tx) = window_stop_tx {
        // WGC path stop.
        let _ = stop_tx.send(()); // triggers capture thread to stop
        // The capture thread will stop on next frame, the frame channel
        // will close, the writer task will close stdin, ffmpeg will see
        // EOF, and the output file will be finalized. Wait for ffmpeg
        // to exit using the existing PID-polling loop.
    }
```

- [ ] **Step 3: Update the 'q' stdin send to skip when it's None**

The existing code:
```rust
    // Extract handles from state BEFORE any await
    let stdin_opt = {
        state.ffmpeg_stdin.lock().map_err(|e| e.to_string())?.take()
    };
    let pid_opt = {
        state.ffmpeg_pid.lock().map_err(|e| e.to_string())?.take()
    };

    // Send 'q' to FFmpeg stdin for graceful stop, then close stdin
    if let Some(mut stdin) = stdin_opt {
        let _ = stdin.write_all(b"q\n").await;
        let _ = stdin.flush().await;
        drop(stdin);
    }
```

This already handles `None` stdin (WGC path has no stdin in AppState), so NO change is needed — the existing `if let Some(mut stdin)` branch is skipped harmlessly.

The PID-polling loop below operates on `pid_opt` which IS populated for both paths, so it works unchanged.

- [ ] **Step 4: Verify compile**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`

Expected: compiles cleanly.

---

## Task 15: Dispatch switch_capture_target by active path

**Files:**
- Modify: `src-tauri/src/commands/recording.rs`

- [ ] **Step 1: Read the current switch_capture_target function**

Read `src-tauri/src/commands/recording.rs` and find `switch_capture_target`. Note that it stops the current ffmpeg via `stop_ffmpeg(stdin_opt, pid_opt, 20)`, pushes the segment, then calls `spawn_ffmpeg` for the new target.

- [ ] **Step 2: Adapt the stop-current-path logic**

At the start of `switch_capture_target`, after the recording-active check, replace:

```rust
    // 2. Extract current stdin and PID from state
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

    // 3-4. Gracefully stop current ffmpeg (poll up to 5s = 20 * 250ms)
    stop_ffmpeg(stdin_opt, pid_opt, 20).await;
```

with:

```rust
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

    // 3-4. Gracefully stop current path. WGC path: send stop signal, then
    // poll for ffmpeg exit. gdigrab path: send 'q' to stdin then poll.
    if let Some(stop_tx) = window_stop_tx {
        let _ = stop_tx.send(());
        // Small settle time so the capture thread notices and closes stdin.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        // Then poll for pid exit using the same helper (stdin is None for
        // this path so stop_ffmpeg just skips the 'q' send).
        stop_ffmpeg(None, pid_opt, 20).await;
    } else {
        stop_ffmpeg(stdin_opt, pid_opt, 20).await;
    }
```

- [ ] **Step 3: Adapt the start-new-path logic**

Further down in `switch_capture_target`, find the section that builds the new output path and calls `spawn_ffmpeg`. Replace it with a dispatch that routes to either `spawn_ffmpeg` (gdigrab) or `window_capture::start_window_capture` (WGC):

Find this block:
```rust
    let result = spawn_ffmpeg(
        &ffmpeg_path,
        &target,
        audio.as_deref(),
        &new_output_str,
    )
    .await?;

    // 8. Update AppState with new PID/stdin/path
    {
        let mut s = state.ffmpeg_stdin.lock().map_err(|e| e.to_string())?;
        *s = result.stdin;
    }
    {
        let mut p = state.ffmpeg_pid.lock().map_err(|e| e.to_string())?;
        *p = result.pid;
    }
```

Replace with:
```rust
    // 7. Start new capture path. Window target -> WGC; others -> gdigrab.
    let (new_pid, new_stdin, new_window_stop) = if matches!(target, CaptureTarget::Window { .. }) {
        #[cfg(target_os = "windows")]
        {
            let handles = crate::commands::window_capture::start_window_capture(
                app.clone(),
                &ffmpeg_path,
                &target,
                audio.as_deref(),
                &new_output_str,
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
```

Note: `result.pid` was previously assigned to a local named `result`; we now rename destructure to `new_pid`/`new_stdin`. Find any subsequent references to `result.pid` in the rest of the function and update them to use `new_pid`.

- [ ] **Step 4: Verify compile**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`

Expected: compiles cleanly. Any remaining `result.pid` references should be caught here.

---

## Task 16: Remove the rect-based Window branch from spawn_ffmpeg

**Files:**
- Modify: `src-tauri/src/commands/recording.rs`

The gdigrab path in `spawn_ffmpeg` no longer handles `CaptureTarget::Window` (Task 12 diverts it before it reaches this function). Remove the now-dead branch to prevent future confusion.

- [ ] **Step 1: Replace the Window branch with unreachable!**

Find this block in `spawn_ffmpeg`:

```rust
        CaptureTarget::Window { rect, .. } => {
            // Use rect-based capture instead of title matching. ...
            args.extend([
                "-f".to_string(),
                "gdigrab".to_string(),
                "-framerate".to_string(),
                "15".to_string(),
                "-offset_x".to_string(),
                rect.x.to_string(),
                "-offset_y".to_string(),
                rect.y.to_string(),
                "-video_size".to_string(),
                format!("{}x{}", rect.width, rect.height),
                "-i".to_string(),
                "desktop".to_string(),
            ]);
        }
```

Replace with:

```rust
        CaptureTarget::Window { .. } => {
            // Window captures go through the WGC path in window_capture.rs
            // and never reach spawn_ffmpeg. If this branch fires, the
            // dispatcher in start_recording or switch_capture_target is
            // broken — fail loud rather than silently do the wrong thing.
            return Err(
                "spawn_ffmpeg called with Window target (dispatcher bug)".to_string(),
            );
        }
```

- [ ] **Step 2: Verify compile**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`

Expected: compiles cleanly.

---

## Task 17: Manual end-to-end verification

No cargo test exists for this code path (Windows APIs + external processes). Verification is manual via `tauri dev`.

- [ ] **Step 1: Run the dev build**

From `C:/Code/happier/portal/agent/`, run:

```
npm run tauri dev
```

Wait for the window to open.

- [ ] **Step 2: Basic window capture test**

1. Open Notepad (any window with a stable ASCII title)
2. In the agent, go to Ready to Record → Window tab
3. Select Notepad from the thumbnail grid
4. Click Start Recording
5. While recording, drag Notepad to a different position on the screen
6. Wait ~5 seconds
7. Click Stop Recording
8. When upload completes, open the recorded file
9. **Pass criteria**: the video shows Notepad at its changing positions — the capture followed the window as it moved

- [ ] **Step 3: Unicode title test (the original bug)**

1. Find a window with a unicode character in its title (e.g., the DevManager window with `•` in the title)
2. Start recording that window
3. Move it around
4. Stop and verify the recording shows the correct window content (not Outlook or whatever else was at the original coordinates)

- [ ] **Step 4: Window close test**

1. Open Notepad
2. Start recording it
3. While recording, close Notepad
4. The agent should automatically stop the recording (triggered by `border:target-closed`)
5. Verify the recorded file is playable (not corrupted)

- [ ] **Step 5: Mid-recording target switch test**

1. Start recording Notepad
2. Click the dock target indicator and switch to a different window (or to Screen mode)
3. Keep recording for a few seconds
4. Stop
5. Verify the final file contains both segments concatenated

- [ ] **Step 6: Audio sync check**

1. Record any window with audio enabled, while talking or playing a known sound
2. Stop recording
3. Play back and confirm audio stays in sync with video

- [ ] **Step 7: Report results**

If any test fails, report the exact failure (what you did, what happened, what you expected). Do NOT claim success without running all tests.

---

## Self-Review Notes

- [x] Spec coverage: all spec sections mapped to tasks (deps → T1, state → T2, module skeleton → T3, ffmpeg builder → T4, spawn → T5, writer → T6, WGC handler → T7, start → T8, stop signal wiring → T9, resize (deferred) → T10, dispatchers → T12–T15, cleanup → T16, verification → T17)
- [x] No "TBD" or "add error handling" placeholders — every code block is complete
- [x] Type consistency: `WindowCaptureHandles`, `HandlerFlags`, `WindowCaptureHandler`, `Frame` names consistent across tasks
- [x] Resize handling deferred explicitly (Task 10) rather than left as a TODO
- [x] Git commits skipped (not a git repo) — per-task verification is `cargo check`

## Known limitations shipped in v1

- **Window resize during recording**: capture continues at original size; new frames are stretched/cropped by the scale filter. Documented in Task 10. Add real resize handling as a follow-up.
- **Windows 10 pre-1903**: WGC is unavailable. The agent already requires modern Windows so this is acceptable.
