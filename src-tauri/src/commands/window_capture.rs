//! Windows Graphics Capture-based window recording.
//!
//! This module captures the MONITOR containing the target window via the
//! WGC WinRT API (through the `windows-capture` crate) and crops each
//! frame in Rust to the current window rect before handing it to ffmpeg.
//!
//! # Why monitor capture instead of window capture
//!
//! WGC has two capture sources: `Window` and `Monitor`. We originally used
//! `Window` because it follows the target across moves/monitor changes,
//! but it has two critical drawbacks for our use case:
//!
//! 1. **It does not see sibling top-level windows.** Our annotation
//!    overlay is a separate Tauri webview that floats over the target;
//!    WGC window-capture does not composite it into the captured frames,
//!    so drawings never appear in the recording. Monitor capture sees
//!    everything the DWM composes onto the display — including our
//!    overlay — exactly as the user sees it.
//!
//! 2. **Frame delivery is tied to window dirty regions.** WGC window
//!    capture only fires `on_frame_arrived` when the target window's
//!    content actually changes. A static window (Notepad with no typing,
//!    a paused video) delivers frames at 2-5 fps even though we configure
//!    a 15 fps target. Output duration becomes dramatically shorter than
//!    wall-clock time. Monitor capture fires at the monitor's refresh
//!    rate (typically 60 Hz) because the DWM composes the whole desktop
//!    on every vsync, so frames arrive steadily and our 15 fps throttle
//!    produces a correctly-paced output file.
//!
//! # Cropping strategy
//!
//! The `watch_window_rect` task (in `recording.rs`) polls `GetWindowRect`
//! every 500ms and writes the current rect into a shared `Arc<Mutex<...>>`
//! held by this module's capture callback. Each incoming monitor frame is
//! cropped to the latest window rect (minus the monitor origin on the
//! virtual desktop). The crop WIDTH/HEIGHT is locked at capture start to
//! the picker's rect dimensions so ffmpeg's `-s WxH` stays valid for the
//! life of the session; only the (x, y) position follows the window. Small
//! window resizes are tolerated (the crop rect keeps its original size,
//! so you get letterboxing or clipping around the edges); large resizes
//! are a known v1 limitation.

#![cfg(target_os = "windows")]

use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;
use tokio::process::{ChildStdin, Command};
use tokio::sync::{mpsc, oneshot};

use windows_capture::{
    capture::{Context, GraphicsCaptureApiHandler},
    frame::Frame as WgcFrame,
    graphics_capture_api::InternalCaptureControl,
    monitor::Monitor as WgcMonitor,
    settings::{
        ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
        MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
    },
    window::Window as WgcWindow,
};

use crate::capture_target::CaptureTarget;

/// One captured frame: raw BGRA bytes in row-major order, top-down.
struct Frame {
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

/// Shared live window rect: `Some((x, y, w, h))` in virtual-desktop
/// physical pixels, or `None` if the window is closed/missing. Updated
/// every ~500ms by `watch_window_rect`; read on every captured frame by
/// the WGC monitor-capture callback.
type RectSlot = Arc<Mutex<Option<(i32, i32, u32, u32)>>>;

/// Data passed from `start_window_capture` into the `new()` constructor of
/// `WindowCaptureHandler` when the capture session spawns.
struct HandlerFlags {
    frame_tx: mpsc::Sender<Frame>,
    app: AppHandle,
    stop_flag: Arc<AtomicBool>,
    /// Live window rect. See `RectSlot`.
    rect_slot: RectSlot,
    /// Virtual-desktop position of the monitor we are capturing. Needed to
    /// translate window rects (which are also in virtual-desktop coords)
    /// into frame-relative crop coordinates.
    monitor_origin: (i32, i32),
    /// Initial window rect from the picker, clamped to the monitor. Used
    /// until the watcher produces its first live update.
    initial_window_rect: (i32, i32, u32, u32),
    /// Fixed crop width (locked at capture start to match ffmpeg's
    /// `-s WxH`). Must be even for yuv420p.
    crop_w: u32,
    /// Fixed crop height. Must be even for yuv420p.
    crop_h: u32,
}

/// WGC capture handler. Each `on_frame_arrived` callback crops the raw
/// BGRA monitor frame down to the current window rect and try_sends the
/// result into the frame channel that feeds ffmpeg's stdin. Drops frames
/// on backpressure rather than blocking the WGC callback thread.
struct WindowCaptureHandler {
    frame_tx: mpsc::Sender<Frame>,
    app: AppHandle,
    stop_flag: Arc<AtomicBool>,
    rect_slot: RectSlot,
    monitor_origin: (i32, i32),
    initial_window_rect: (i32, i32, u32, u32),
    crop_w: u32,
    crop_h: u32,
    last_frame_at: Option<Instant>,
    /// Scratch buffer for the cropped output. Reused across frames to
    /// avoid allocating a fresh Vec on every callback.
    crop_buf: Vec<u8>,
    /// Monitor frame dimensions seen on the most recent frame, for
    /// one-time logging.
    logged_monitor_dims: Option<(u32, u32)>,
    resize_logged: bool,
}

const TARGET_FPS: u64 = 15;
const MIN_FRAME_INTERVAL: Duration = Duration::from_millis(1000 / TARGET_FPS);

impl GraphicsCaptureApiHandler for WindowCaptureHandler {
    type Flags = HandlerFlags;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        let flags = ctx.flags;
        let crop_buf_size = (flags.crop_w as usize) * (flags.crop_h as usize) * 4;
        Ok(Self {
            frame_tx: flags.frame_tx,
            app: flags.app,
            stop_flag: flags.stop_flag,
            rect_slot: flags.rect_slot,
            monitor_origin: flags.monitor_origin,
            initial_window_rect: flags.initial_window_rect,
            crop_w: flags.crop_w,
            crop_h: flags.crop_h,
            last_frame_at: None,
            crop_buf: vec![0u8; crop_buf_size],
            logged_monitor_dims: None,
            resize_logged: false,
        })
    }

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

        // Throttle: drop frames that arrive too fast for our 15fps target.
        // Monitor capture fires at the monitor's refresh rate (~60 Hz),
        // which is four times our target — without this the encoder
        // channel fills up instantly and we drop frames at random.
        let now = Instant::now();
        if let Some(prev) = self.last_frame_at {
            if now.duration_since(prev) < MIN_FRAME_INTERVAL {
                return Ok(());
            }
        }
        self.last_frame_at = Some(now);

        let monitor_w = frame.width();
        let monitor_h = frame.height();

        // One-time log so we know what monitor dimensions WGC delivers
        // relative to what the Win32 API reported for the picker rect.
        if self.logged_monitor_dims.is_none() {
            eprintln!(
                "[window_capture] first monitor frame: {}x{} (crop target: {}x{})",
                monitor_w, monitor_h, self.crop_w, self.crop_h
            );
            self.logged_monitor_dims = Some((monitor_w, monitor_h));
        }

        // Look up the current window rect from the shared slot updated by
        // `watch_window_rect`. If the watcher has not run yet (or failed
        // to get a rect), fall back to the initial rect captured at
        // session start.
        let live_rect = {
            let guard = self.rect_slot.lock().ok();
            match guard.as_ref().and_then(|g| **g) {
                Some(r) => r,
                None => self.initial_window_rect,
            }
        };
        let (win_x, win_y, win_w, win_h) = live_rect;

        if (win_w != self.crop_w || win_h != self.crop_h) && !self.resize_logged {
            // Log once — we don't adapt the crop size mid-recording
            // because ffmpeg's `-s WxH` is fixed. Large resizes will
            // produce clipping (if the window grew) or black padding
            // (if it shrank). This is a known v1 limitation.
            eprintln!(
                "[window_capture] window resized from {}x{} to {}x{} — \
                crop size stays locked (v1 limitation)",
                self.crop_w, self.crop_h, win_w, win_h
            );
            self.resize_logged = true;
        }

        // Translate window position from virtual-desktop coordinates to
        // monitor-frame-relative coordinates by subtracting the monitor's
        // origin.
        let crop_x = win_x - self.monitor_origin.0;
        let crop_y = win_y - self.monitor_origin.1;

        // Copy the raw BGRA bytes out of the WGC frame. `as_nopadding_buffer`
        // strips any DXGI row padding so the source slice is tightly packed
        // at monitor_w * 4 bytes per row.
        let mut buffer = frame.buffer()?;
        let source = buffer.as_nopadding_buffer()?;
        let expected_source_len = (monitor_w as usize) * (monitor_h as usize) * 4;
        if source.len() != expected_source_len {
            eprintln!(
                "[window_capture] WARN: source buffer size {} != expected {} \
                (monitor {}x{})",
                source.len(),
                expected_source_len,
                monitor_w,
                monitor_h
            );
            // Skip this frame rather than risk reading past the buffer.
            return Ok(());
        }

        // Perform the crop into our reusable scratch buffer.
        crop_bgra_frame(
            source,
            monitor_w,
            monitor_h,
            crop_x,
            crop_y,
            self.crop_w,
            self.crop_h,
            &mut self.crop_buf,
        );

        // Clone the scratch buffer into a fresh Vec for the channel. The
        // crop_buf stays allocated at its initial capacity and is zeroed
        // at the top of the next crop call.
        let owned = Frame {
            data: self.crop_buf.clone(),
        };

        // Non-blocking send with drop-on-full semantics (backpressure).
        // If the encoder is behind, drop this frame rather than blocking
        // the capture thread — blocking here would stall the DWM.
        if self.frame_tx.try_send(owned).is_err() {
            // Encoder is behind. Drop this frame silently; logging here
            // would spam stderr during any slow ffmpeg start.
        }

        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        // The WGC item closed unexpectedly — for monitor capture this
        // happens when the monitor is disconnected. Emit the target-closed
        // event so the frontend stops the recording.
        let _ = self.app.emit("border:target-closed", ());
        Ok(())
    }
}

/// A second WGC session, bound to the TARGET WINDOW, that exists only so
/// DWM draws its native yellow "this window is being captured" border
/// around the window at compositor level. We don't record its frames —
/// we just need the session to be active so the border appears. The
/// actual recording comes from the monitor-capture session above.
///
/// This is the same trick Teams/Slack use: window-targeted capture
/// session for the border UX, independent of what's actually being
/// encoded. Keeps the border pixel-perfect-follows-window even when
/// we crop from a larger source.
struct BorderOnlyHandler {
    stop_flag: Arc<AtomicBool>,
}

struct BorderOnlyFlags {
    stop_flag: Arc<AtomicBool>,
}

impl GraphicsCaptureApiHandler for BorderOnlyHandler {
    type Flags = BorderOnlyFlags;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            stop_flag: ctx.flags.stop_flag,
        })
    }

    fn on_frame_arrived(
        &mut self,
        _frame: &mut WgcFrame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        // Deliberately discard frames — we're only here for the DWM border.
        // We intentionally do NOT call frame.buffer() so the crate can skip
        // copying the surface to CPU memory.
        if self.stop_flag.load(Ordering::Relaxed) {
            capture_control.stop();
        }
        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// Crop a region out of a BGRA source frame into `dest`. The destination
/// buffer is zeroed before writing so any out-of-bounds region (when the
/// window partially leaves the monitor) stays black rather than showing
/// stale pixels from the previous frame. `crop_x` and `crop_y` may be
/// negative (window extends off the top/left of the monitor) and the
/// crop rect may extend past the monitor edge; only the overlapping
/// region is copied.
fn crop_bgra_frame(
    source: &[u8],
    source_width: u32,
    source_height: u32,
    crop_x: i32,
    crop_y: i32,
    crop_width: u32,
    crop_height: u32,
    dest: &mut Vec<u8>,
) {
    let dest_bytes = (crop_width as usize) * (crop_height as usize) * 4;
    if dest.len() != dest_bytes {
        dest.resize(dest_bytes, 0);
    }
    // Zero the whole destination so off-screen regions read as black.
    // We can't just skip rows/columns outside the overlap because the
    // previous frame's data would leak through.
    for b in dest.iter_mut() {
        *b = 0;
    }

    let source_stride = (source_width as usize) * 4;
    let dest_stride = (crop_width as usize) * 4;

    // X-overlap, computed once outside the row loop.
    let sx_start = crop_x.max(0);
    let sx_end = (crop_x + crop_width as i32).min(source_width as i32);
    if sx_end <= sx_start {
        return; // fully off-screen horizontally
    }
    let copy_width_px = (sx_end - sx_start) as usize;
    let copy_bytes = copy_width_px * 4;
    let dx_start = (sx_start - crop_x) as usize;
    let dx_byte_offset = dx_start * 4;
    let sx_byte_offset = (sx_start as usize) * 4;

    for dy in 0..crop_height as i32 {
        let sy = crop_y + dy;
        if sy < 0 || sy >= source_height as i32 {
            continue;
        }
        let dest_row_start = (dy as usize) * dest_stride + dx_byte_offset;
        let source_row_start = (sy as usize) * source_stride + sx_byte_offset;
        dest[dest_row_start..dest_row_start + copy_bytes]
            .copy_from_slice(&source[source_row_start..source_row_start + copy_bytes]);
    }
}

/// Resolve the monitor that contains the point `(x, y)` in virtual-desktop
/// coordinates and return both the `WgcMonitor` (for WGC capture) and the
/// monitor's origin `(left, top)` (for crop coordinate translation).
fn resolve_monitor_for_point(x: i32, y: i32) -> Result<(WgcMonitor, (i32, i32)), String> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };

    let point = POINT { x, y };
    // SAFETY: MonitorFromPoint is a pure Win32 lookup with no preconditions
    // on its arguments. It always returns a valid HMONITOR when
    // MONITOR_DEFAULTTONEAREST is set (never NULL).
    let hmonitor: HMONITOR = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
    if hmonitor.is_invalid() {
        return Err("MonitorFromPoint returned invalid handle".to_string());
    }

    // Query the monitor's virtual-desktop rect to get its origin.
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is zero-initialized with cbSize set; the HMONITOR is
    // valid per the check above.
    let ok = unsafe { GetMonitorInfoW(hmonitor, &mut info).as_bool() };
    if !ok {
        return Err("GetMonitorInfoW failed for resolved HMONITOR".to_string());
    }
    let origin = (info.rcMonitor.left, info.rcMonitor.top);

    // Wrap the raw HMONITOR as a windows-capture Monitor. The crate's
    // `from_raw_hmonitor` is a const fn that just stashes the handle; it
    // does no additional validation and we already checked is_invalid.
    let wgc_monitor = WgcMonitor::from_raw_hmonitor(hmonitor.0);

    Ok((wgc_monitor, origin))
}

/// Public entry point. Starts a WGC monitor-capture session for the
/// monitor that contains the target window, and an ffmpeg child that
/// reads raw BGRA frames (cropped to the window) from stdin. Returns the
/// handles needed to stop the session and the output file path.
pub async fn start_window_capture(
    app: AppHandle,
    ffmpeg_path: &Path,
    target: &CaptureTarget,
    audio: Option<&str>,
    output_path: &str,
    rect_slot: RectSlot,
) -> Result<WindowCaptureHandles, String> {
    // Extract HWND, title, and the initial picker rect.
    let (hwnd, title, rect) = match target {
        CaptureTarget::Window { hwnd, title, rect } => (*hwnd, title.clone(), rect.clone()),
        _ => return Err("start_window_capture called with non-Window target".to_string()),
    };

    eprintln!(
        "[window_capture] starting WGC monitor-capture session: \
         hwnd={} (0x{:X}), title={:?}, initial rect=({}, {}) {}x{}",
        hwnd, hwnd, title, rect.x, rect.y, rect.width, rect.height
    );

    // Resolve the monitor containing the window's top-left corner. Using
    // the window's origin (rather than its center) is fine for any window
    // that isn't straddling two monitors — which is the common case — and
    // MonitorFromPoint's DEFAULTTONEAREST fallback handles the edge case.
    let (wgc_monitor, monitor_origin) = resolve_monitor_for_point(rect.x, rect.y)?;
    eprintln!(
        "[window_capture] target monitor origin=({}, {})",
        monitor_origin.0, monitor_origin.1
    );

    // Lock the crop dimensions at session start. ffmpeg's `-s WxH` is
    // fixed for the life of the process, and yuv420p requires even
    // dimensions for chroma subsampling — so round DOWN to the nearest
    // even pixel. A 1-pixel shrink is invisible; the alternative
    // (rounding up) could overshoot the window and reveal stray pixels.
    let crop_w = rect.width & !1u32;
    let crop_h = rect.height & !1u32;
    if crop_w == 0 || crop_h == 0 {
        return Err(format!(
            "Window rect is too small to record: {}x{}",
            rect.width, rect.height
        ));
    }
    eprintln!(
        "[window_capture] crop dimensions locked at {}x{} (rounded from {}x{})",
        crop_w, crop_h, rect.width, rect.height
    );

    // Bounded frame channel. Capacity 15 = ~1 second of buffer at 15fps,
    // which is plenty to smooth over small ffmpeg stalls without letting
    // backpressure build up into multi-second latency.
    let (frame_tx, frame_rx) = mpsc::channel::<Frame>(15);
    // Stop signal: caller sends on this to stop the capture thread.
    let (stop_tx, stop_rx) = oneshot::channel::<()>();

    // Shared stop flag for the main monitor-capture session.
    let stop_flag = Arc::new(AtomicBool::new(false));
    // Shared stop flag for the border-only window-capture session.
    // Created here so the single bridge task below can flip both when
    // the outer stop signal fires.
    let border_stop_flag = Arc::new(AtomicBool::new(false));

    let stop_flag_for_bridge = stop_flag.clone();
    let border_stop_flag_for_bridge = border_stop_flag.clone();
    tokio::spawn(async move {
        let _ = stop_rx.await;
        stop_flag_for_bridge.store(true, Ordering::Relaxed);
        border_stop_flag_for_bridge.store(true, Ordering::Relaxed);
    });

    // Prime the shared rect slot with the initial picker rect so the
    // capture callback has a valid crop target on the very first frame,
    // before the 500ms watcher has run even once. The watcher (called
    // from the frontend immediately after start_recording) will overwrite
    // this on its first tick.
    {
        let initial = (rect.x, rect.y, rect.width, rect.height);
        if let Ok(mut slot) = rect_slot.lock() {
            *slot = Some(initial);
        }
    }
    let initial_window_rect = (rect.x, rect.y, rect.width, rect.height);

    // Spawn ffmpeg FIRST with the known crop dimensions. With window
    // capture we used to wait for the first WGC frame to confirm the
    // delivered size (because WGC sometimes delivered a different size
    // than GetWindowRect reported), but now ffmpeg reads cropped frames
    // at a fixed size we control, so the first-frame wait is obsolete.
    let (ffmpeg_pid, stdin) =
        spawn_ffmpeg_child(ffmpeg_path, crop_w, crop_h, audio, output_path).await?;
    eprintln!(
        "[window_capture] ffmpeg spawned (pid {:?}) at {}x{}",
        ffmpeg_pid, crop_w, crop_h
    );

    // Build the WGC settings. Key differences vs. the old window-capture
    // path:
    //
    //   * Item is now `wgc_monitor`, not a window — sees everything DWM
    //     composes onto the display, including our annotation overlay.
    //   * DrawBorderSettings::WithoutBorder — with window capture this
    //     drew a yellow outline around the target window; with monitor
    //     capture it would draw around the ENTIRE monitor, which is
    //     neither useful nor desired. We render our own Tauri border
    //     overlay on the frontend instead.
    let settings = Settings::new(
        wgc_monitor,
        CursorCaptureSettings::WithCursor,
        DrawBorderSettings::WithoutBorder,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Bgra8,
        HandlerFlags {
            frame_tx,
            app: app.clone(),
            stop_flag,
            rect_slot,
            monitor_origin,
            initial_window_rect,
            crop_w,
            crop_h,
        },
    );

    // Spawn the capture session on its own OS thread. `start` blocks the
    // calling thread with a WinRT message loop until the capture stops.
    std::thread::spawn(move || {
        if let Err(e) = WindowCaptureHandler::start(settings) {
            eprintln!("[window_capture] capture session error: {:?}", e);
        }
    });

    // ─── Second WGC session: border-only ─────────────────────────────────
    //
    // Monitor capture doesn't carry the "this window is being captured"
    // DWM border — that's only drawn for Window-source sessions. To get
    // the native Teams/Slack-style yellow border that follows the window
    // pixel-perfectly, spawn a second WGC session bound directly to the
    // target window with DrawBorderSettings::WithBorder. We discard its
    // frames (the monitor session provides the recording bytes), so the
    // only cost is keeping the session alive for the duration of the
    // recording.
    //
    // `WgcWindow::from_raw_hwnd` returns a `Window` directly (not a
    // Result). If the hwnd later turns out to be invalid, the capture
    // session start call will return an error which we log and swallow.
    let border_window = WgcWindow::from_raw_hwnd(hwnd as *mut std::ffi::c_void);
    let border_settings = Settings::new(
        border_window,
        CursorCaptureSettings::WithoutCursor,
        DrawBorderSettings::WithBorder,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Bgra8,
        BorderOnlyFlags {
            stop_flag: border_stop_flag,
        },
    );
    std::thread::spawn(move || {
        if let Err(e) = BorderOnlyHandler::start(border_settings) {
            eprintln!("[window_capture] border session error: {:?}", e);
        }
    });
    eprintln!("[window_capture] border session spawned for hwnd 0x{:X}", hwnd);

    // Spawn the writer task. It drains the frame channel and writes the
    // cropped BGRA bytes to ffmpeg's stdin.
    tokio::spawn(run_frame_writer(frame_rx, stdin));

    Ok(WindowCaptureHandles {
        stop_tx,
        ffmpeg_pid,
        output_path: output_path.to_string(),
    })
}

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

    // Raw BGRA video from stdin.
    //
    // `-use_wallclock_as_timestamps 1` on the input tells ffmpeg to
    // timestamp each incoming frame using arrival wall clock time,
    // preserving the real duration regardless of irregular arrival.
    // With monitor capture + 15 fps throttle, frames SHOULD arrive at
    // a fairly steady 15 Hz, but a brief ffmpeg stall could still cause
    // us to drop a frame or two — wallclock timestamps keep the output
    // duration accurate to wall-clock regardless.
    //
    // `-vsync cfr -r 15` on the output resamples the input into a
    // constant 15fps output, duplicating frames over any gaps.
    args.extend([
        "-use_wallclock_as_timestamps".into(),
        "1".into(),
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
        // Constant frame rate at 15fps — ffmpeg duplicates/drops frames
        // to match wall-clock timing from `use_wallclock_as_timestamps`.
        "-vsync".into(),
        "cfr".into(),
        "-r".into(),
        "15".into(),
    ]);

    if !audio_device.is_empty() && audio_device != "none" {
        args.extend(["-c:a".into(), "libopus".into()]);
    } else {
        args.extend(["-an".into()]);
    }

    // -shortest ensures ffmpeg stops when the shorter of audio/video ends.
    args.extend(["-shortest".into(), "-y".into(), output_path.to_string()]);

    args
}

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
        .unwrap_or_else(|| std::env::temp_dir().join("prospertogether"));
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
