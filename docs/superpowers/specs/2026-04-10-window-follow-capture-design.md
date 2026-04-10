# Window-Follow Capture Design

**Date**: 2026-04-10
**Status**: Approved design, pending implementation plan

## Context

The current window capture implementation uses gdigrab with rect-based capture (`-offset_x/-offset_y/-video_size -i desktop`). This works but has a critical flaw: **the capture area is fixed at the moment recording starts**. If the user moves the window during recording, the recording shows whatever is at the original screen coordinates — not the window they wanted to capture.

Users routinely move windows around while working. Teams, Slack, OBS, and other modern screen recorders support window-follow capture as a standard feature. Our current implementation does not.

### Why the obvious alternatives don't work

- **`gdigrab -i title=...`**: FFmpeg's gdigrab uses `FindWindowA` (ANSI) for window lookup, so any non-ASCII character in the title (e.g., the `•` in "Dev Agent • window-region-capture-picker • DevManager") breaks matching. Even with sanitized titles, `FindWindow` requires an exact match and fails with duplicate titles.
- **Restart ffmpeg on every window move**: Creates visible 1-2s gaps every time the user nudges the window. Users move windows constantly. The stuttering is unacceptable.
- **Record the full desktop and post-crop**: Possible with `sendcmd` + `crop` filter, but captures everything outside the window (privacy concern, larger files) and requires a full second ffmpeg pass scaling with recording length.

## Goal

Implement Teams/Slack-quality window capture that automatically follows the window as it moves across the screen during recording, without visible artifacts.

## Design

### Approach: Windows Graphics Capture (WGC)

Use the `windows-capture` Rust crate, which wraps the Windows.Graphics.Capture WinRT API — the same API Teams, Slack, OBS, and Microsoft's own Game Bar use. WGC captures a specific window by HWND and automatically follows it through moves, monitor changes, and occlusion. Frames come out of the compositor, not the desktop framebuffer, so they are pixel-perfect even when the window is partially off-screen.

Requires Windows 10 1903+ (May 2019). Older Windows is not supported — acceptable since the app already relies on modern APIs.

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│ start_recording(target=Window{hwnd, rect})                       │
│                                                                  │
│  ┌──────────────────┐     ┌────────────────┐    ┌─────────────┐ │
│  │ WGC Session      │     │ Frame channel  │    │ ffmpeg      │ │
│  │ (own thread)     │ --> │ (tokio mpsc,   │ -> │ child proc  │ │
│  │  bound to HWND   │     │  bounded)      │    │             │ │
│  └──────────────────┘     └────────────────┘    └─────────────┘ │
│        |                                               |        │
│        | on_closed event                               v        │
│        v                                       recording.webm   │
│  border:target-closed                                            │
│                                                                  │
│  ┌──────────────────┐                                            │
│  │ Audio: dshow     │ ─────────────────────────────────┘         │
│  └──────────────────┘                                            │
└─────────────────────────────────────────────────────────────────┘
```

1. `start_recording` receives `CaptureTarget::Window { hwnd, rect, .. }`
2. Spawns ffmpeg child process with two inputs: dshow audio + rawvideo stdin pipe
3. Spawns a WGC capture session bound to the HWND on its own thread
4. The capture frame callback receives BGRA frames at the window's current native size, throttles to 15fps, and pushes to a bounded tokio mpsc channel
5. An async writer task drains the channel and writes raw BGRA bytes to ffmpeg's stdin
6. When the user moves the window → WGC keeps delivering frames from the window's current position → no interruption
7. When the user resizes the window → WGC emits a size-change event → we restart ffmpeg with the new `-s WxH` (new segment in the existing segment-management flow)
8. When the window closes → WGC emits `on_closed` → we emit `border:target-closed` to the frontend → frontend triggers `handleStop`

### Module layout

```
src-tauri/src/commands/
  recording.rs         # existing; dispatches to either path
  window_capture.rs    # new; WGC session + frame channel + ffmpeg pipe writer
```

`recording.rs::start_recording` becomes a dispatcher:

```rust
match target {
    CaptureTarget::Window { .. } => {
        window_capture::start_window_capture(app, state, target, audio).await
    }
    _ => {
        // existing spawn_ffmpeg path for Screen/Monitor/Region
    }
}
```

`stop_recording` dispatches based on which path is active (tracked in `AppState`).

### FFmpeg invocation for window capture

```
ffmpeg \
  -f dshow -i audio={device} \
  -f rawvideo -pix_fmt bgra -s {width}x{height} -framerate 15 -i pipe:0 \
  -vf scale='min(1920,iw):-2' \
  -pix_fmt yuv420p \
  -c:v libvpx -b:v 1M -deadline realtime -cpu-used 8 \
  -c:a libopus \
  -shortest \
  -y output.webm
```

Key differences from the existing gdigrab path:
- Video input is `-f rawvideo -pix_fmt bgra -s {w}x{h} -framerate 15 -i pipe:0` instead of `-f gdigrab ...`
- `-shortest` flag ensures ffmpeg stops when the shorter of audio/video ends
- `{width}x{height}` is the window's current size in physical pixels at the moment capture starts

### Frame throttling

WGC delivers frames asynchronously as the compositor renders them — potentially 60fps+ on an active window. We throttle in the frame callback:

```rust
let min_interval = Duration::from_millis(66); // ~15fps
let now = Instant::now();
if now.duration_since(last_frame_time) < min_interval {
    return; // drop frame
}
last_frame_time = now;
// copy frame bytes, push to channel
```

### Channel and backpressure

Use a bounded `tokio::sync::mpsc` channel with capacity ~3 frames. If ffmpeg falls behind encoding, the frame callback's `try_send` fails and we drop the frame. Prevents unbounded memory growth if encoding can't keep up.

### AppState additions

```rust
pub struct AppState {
    // ...existing fields...

    /// Handle to the WGC capture session control channel. Sending a unit on
    /// this stops the capture thread cleanly. None when no window capture
    /// is in progress (i.e. in Screen/Monitor/Region mode or idle).
    pub window_capture_stop: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}
```

### Graceful stop flow

Current `stop_recording` sends `'q'` to ffmpeg's stdin. That no longer works for window capture because stdin is now the frame byte stream. New unified stop flow:

1. **If window capture is active** (`window_capture_stop` is Some):
   - Send the stop signal on the oneshot channel
   - The capture thread stops polling WGC and closes the frame channel sender
   - The writer task sees the channel close, closes ffmpeg's stdin, and exits
   - ffmpeg hits EOF on rawvideo input → finalizes output due to `-shortest` flag
2. **Otherwise** (gdigrab path): send `'q'` to stdin as today
3. **Common tail**: poll for ffmpeg exit every 250ms up to 10s, then taskkill

### Window resize handling

WGC emits a frame with updated dimensions when the window is resized. In the frame callback:

```rust
if frame.width != current_width || frame.height != current_height {
    // Window was resized. Trigger an ffmpeg restart via the same
    // mechanism used by switch_capture_target: stop current ffmpeg,
    // push current segment to recording_segments, start new ffmpeg
    // with new dimensions, resume frame streaming into it.
}
```

The existing segment-concat logic in `stop_recording` handles the resulting multi-segment file. Users who resize see a brief glitch (same as mid-recording target switch); users who only move see nothing.

### Window close handling

WGC provides an `on_closed` callback. When fired:
1. Emit `border:target-closed` Tauri event (same event the existing window watcher uses)
2. Close the frame channel to trigger graceful shutdown
3. Frontend's existing listener calls `handleStop`

The existing `watch_window_rect` command for the border overlay stays as-is — it's still needed to reposition the border visual as the window moves. WGC handles capture; the watcher handles the border overlay.

### Things that stay unchanged

- Border overlay (`useBorderOverlay.ts`, `BorderPage.tsx`) and window rect watcher in Rust — still drive the visual border that follows the window. The watcher uses `GetWindowRect` polling independent of WGC; the border follows the window via watcher events, while WGC drives the capture independently.
- Segment management + concat in `stop_recording` — window resize now generates segments too
- Drawing overlay resized to window bounds — unchanged
- All non-Window capture modes (Screen, Monitor, Region) — unchanged gdigrab path
- `CaptureTarget::Window { hwnd, title, rect }` — shape unchanged. The `rect` is still used as the initial border overlay position and in the window picker thumbnail. It is no longer passed to ffmpeg.

### Mid-recording target switching across capture paths

`switch_capture_target` must now handle switching between capture paths (gdigrab ↔ WGC). Flow:

1. Stop whichever path is currently running (close frame channel for WGC, or send 'q' to stdin for gdigrab)
2. Wait for ffmpeg to exit
3. Push the completed segment path to `recording_segments`
4. Start the new path (WGC for Window target, gdigrab otherwise) with a new output filename
5. Update `AppState` to reflect the new path's handles

The segment concat at stop time is path-agnostic — segments from gdigrab and WGC are both standard VP8 webm files and can be concatenated with `-c copy`.

### Things that go away

- The rect-based `CaptureTarget::Window` branch in `spawn_ffmpeg` — window mode no longer uses gdigrab at all
- Any remaining confusion about what `gdigrab -i title=...` does — we never use it
- The disclaimer that window capture doesn't follow movement

### Dependency

Add to `Cargo.toml` under `[target.'cfg(target_os = "windows")'.dependencies]`:
```toml
windows-capture = "1.4"
```

The crate has its own runtime for the WGC message loop and wraps the WinRT COM interop. It uses the same `windows` crate we already depend on.

## Error handling

| Scenario | Handling |
|----------|----------|
| HWND invalid at capture start | Return error from `start_window_capture`, frontend shows error, stays in idle state |
| HWND becomes invalid during recording (window closed) | `on_closed` callback → emit `border:target-closed` → frontend stops recording → segment(s) saved |
| WGC capture create fails (unsupported Windows version) | Return error with message asking user to upgrade to Windows 10 1903+ |
| ffmpeg spawn fails | Return error, cleanup capture session, stay idle |
| Frame channel full (ffmpeg backpressure) | Drop oldest frame, log warning. Acceptable for 15fps target. |
| Window resize during recording | Segment restart (see above) |
| Stop signal received during active capture | Flush pending frames, close stdin, wait for ffmpeg, return output path |

## Testing approach

Can't run the code end-to-end without a physical Windows machine, so verification will be:

1. **Compile check**: `cargo check --target x86_64-pc-windows-msvc`
2. **Type check**: confirm `windows-capture` types match usage
3. **Integration test with a known window**: Launch Notepad (trivial ASCII title, stable HWND), record for 10s while moving it around, verify the output video shows Notepad the whole time at its changing positions
4. **Resize test**: Record a resizable window (e.g., a browser), resize mid-recording, verify segments are produced and concatenated into a playable file
5. **Close test**: Close the target window during recording, verify recording stops gracefully and the file is playable
6. **DPI test**: Record a window on a HiDPI monitor (150%+), verify the output is correctly scaled
7. **Audio sync**: Confirm audio stays in sync with video through multiple segments (concat with `-c copy` should preserve timing)

## Out of scope

- Pre-windows-10-1903 compatibility — we don't support it
- Capturing child windows or UI elements — only top-level windows
- Capturing protected content (DRM-protected video players) — WGC can't, nothing we can do
- Hardware-encoded output — future optimization, stick with libvpx for now

## Open questions

None blocking. The design is ready for planning.
