use serde::Serialize;
use tokio::process::Command;

#[derive(Serialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub device_type: String,
}

// ── Monitor / Window types ─────────────────────────────────────────────

#[derive(Serialize, Clone)]
pub struct MonitorInfo {
    pub id: u32,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

#[derive(Serialize, Clone)]
pub struct WindowInfo {
    pub hwnd: isize,
    pub title: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub thumbnail: String,
}

// ── Windows implementation ─────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod win {
    use super::*;
    use base64::Engine as _;
    use image::codecs::png::PngEncoder;
    use image::ImageEncoder;
    use std::io::Cursor;
    use std::mem;
    use windows::Win32::Foundation::*;
    use windows::Win32::Graphics::Dwm::*;
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    // PrintWindow is a User32 function. Declare via FFI to avoid
    // module-location ambiguity across windows-rs versions.
    #[link(name = "user32")]
    extern "system" {
        fn PrintWindow(hwnd: HWND, hdc: HDC, nflags: u32) -> BOOL;
    }
    const PW_RENDERFULLCONTENT: u32 = 0x00000002;

    // ── list_monitors ──────────────────────────────────────────────────

    unsafe extern "system" fn monitor_enum_proc(
        hmonitor: HMONITOR,
        _hdc: HDC,
        _lprc_clip: *mut RECT,
        lparam: LPARAM,
    ) -> BOOL {
        let monitors = &mut *(lparam.0 as *mut Vec<MonitorInfo>);

        let mut mi: MONITORINFOEXW = mem::zeroed();
        mi.monitorInfo.cbSize = mem::size_of::<MONITORINFOEXW>() as u32;

        if GetMonitorInfoW(hmonitor, &mut mi.monitorInfo as *mut _ as *mut MONITORINFO).as_bool() {
            let rc = mi.monitorInfo.rcMonitor;
            let primary = (mi.monitorInfo.dwFlags & MONITORINFOF_PRIMARY) != 0;
            let idx = monitors.len() as u32 + 1;
            monitors.push(MonitorInfo {
                id: idx,
                name: format!("Monitor {}", idx),
                x: rc.left,
                y: rc.top,
                width: (rc.right - rc.left) as u32,
                height: (rc.bottom - rc.top) as u32,
                primary,
            });
        }

        BOOL(1) // continue enumeration
    }

    pub fn enumerate_monitors() -> Result<Vec<MonitorInfo>, String> {
        let mut monitors: Vec<MonitorInfo> = Vec::new();
        unsafe {
            let ok = EnumDisplayMonitors(
                None,
                None,
                Some(monitor_enum_proc),
                LPARAM(&mut monitors as *mut Vec<MonitorInfo> as isize),
            );
            if !ok.as_bool() {
                return Err("EnumDisplayMonitors failed".to_string());
            }
        }
        Ok(monitors)
    }

    // ── list_windows helpers ───────────────────────────────────────────

    fn get_window_title(hwnd: HWND) -> Option<String> {
        unsafe {
            let len = GetWindowTextLengthW(hwnd);
            if len == 0 {
                return None;
            }
            let mut buf = vec![0u16; (len + 1) as usize];
            let copied = GetWindowTextW(hwnd, &mut buf);
            if copied == 0 {
                return None;
            }
            Some(String::from_utf16_lossy(&buf[..copied as usize]))
        }
    }

    fn is_cloaked(hwnd: HWND) -> bool {
        unsafe {
            let mut cloaked: u32 = 0;
            let hr = DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                &mut cloaked as *mut u32 as *mut _,
                mem::size_of::<u32>() as u32,
            );
            hr.is_ok() && cloaked != 0
        }
    }

    fn is_tool_window(hwnd: HWND) -> bool {
        unsafe {
            let style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            (style & WS_EX_TOOLWINDOW.0) != 0
        }
    }

    /// Capture a window thumbnail via PrintWindow, scale it down, and return
    /// a `data:image/png;base64,...` string. Returns empty string on failure.
    fn capture_thumbnail(hwnd: HWND) -> String {
        const MAX_THUMB_W: u32 = 240;

        unsafe {
            // Get window rect for dimensions
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_err() {
                return String::new();
            }

            let w = (rect.right - rect.left) as i32;
            let h = (rect.bottom - rect.top) as i32;
            if w <= 0 || h <= 0 {
                return String::new();
            }
            let w = w as u32;
            let h = h as u32;

            // Create memory DC + bitmap
            let screen_dc = GetDC(None);
            if screen_dc.is_invalid() {
                return String::new();
            }
            let mem_dc = CreateCompatibleDC(screen_dc);
            if mem_dc.is_invalid() {
                ReleaseDC(None, screen_dc);
                return String::new();
            }
            let bitmap = CreateCompatibleBitmap(screen_dc, w as i32, h as i32);
            if bitmap.is_invalid() {
                let _ = DeleteDC(mem_dc);
                ReleaseDC(None, screen_dc);
                return String::new();
            }
            let old_bmp = SelectObject(mem_dc, bitmap);

            let ok = PrintWindow(hwnd, mem_dc, PW_RENDERFULLCONTENT);
            if !ok.as_bool() {
                // Fallback: PrintWindow failed (DX window, elevated, etc.)
                SelectObject(mem_dc, old_bmp);
                let _ = DeleteObject(bitmap);
                let _ = DeleteDC(mem_dc);
                ReleaseDC(None, screen_dc);
                return String::new();
            }

            // Read raw bits via GetDIBits
            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w as i32,
                    biHeight: -(h as i32), // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0 as u32,
                    ..mem::zeroed()
                },
                ..mem::zeroed()
            };

            let row_bytes = w * 4;
            let buf_size = (row_bytes * h) as usize;
            let mut pixels = vec![0u8; buf_size];

            let lines = GetDIBits(
                mem_dc,
                bitmap,
                0,
                h,
                Some(pixels.as_mut_ptr() as *mut _),
                &mut bmi,
                DIB_RGB_COLORS,
            );

            // Cleanup GDI
            SelectObject(mem_dc, old_bmp);
            let _ = DeleteObject(bitmap);
            let _ = DeleteDC(mem_dc);
            ReleaseDC(None, screen_dc);

            if lines == 0 {
                return String::new();
            }

            // Convert BGRA -> RGBA
            for chunk in pixels.chunks_exact_mut(4) {
                chunk.swap(0, 2); // B <-> R
            }

            // Build an image buffer
            let img_buf = match image::RgbaImage::from_raw(w, h, pixels) {
                Some(buf) => buf,
                None => return String::new(),
            };

            // Scale down if wider than MAX_THUMB_W
            let final_img = if w > MAX_THUMB_W {
                let scale = MAX_THUMB_W as f64 / w as f64;
                let new_h = (h as f64 * scale).round() as u32;
                image::imageops::resize(
                    &img_buf,
                    MAX_THUMB_W,
                    new_h,
                    image::imageops::FilterType::Triangle,
                )
            } else {
                img_buf
            };

            // Encode to PNG
            let mut png_bytes: Vec<u8> = Vec::new();
            {
                let cursor = Cursor::new(&mut png_bytes);
                let encoder = PngEncoder::new(cursor);
                if encoder
                    .write_image(
                        final_img.as_raw(),
                        final_img.width(),
                        final_img.height(),
                        image::ExtendedColorType::Rgba8,
                    )
                    .is_err()
                {
                    return String::new();
                }
            }

            // Base64 encode
            let b64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
            format!("data:image/png;base64,{}", b64)
        }
    }

    struct EnumWindowsData {
        own_pid: u32,
        windows: Vec<WindowInfo>,
    }

    unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let data = &mut *(lparam.0 as *mut EnumWindowsData);

        // Must be visible
        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }

        // Must have a title
        if get_window_title(hwnd).is_none() {
            return BOOL(1);
        }

        // Skip cloaked windows (UWP hidden, virtual desktops)
        if is_cloaked(hwnd) {
            return BOOL(1);
        }

        // Skip tool windows
        if is_tool_window(hwnd) {
            return BOOL(1);
        }

        // Skip our own process windows
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == data.own_pid {
            return BOOL(1);
        }

        // Get rect
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return BOOL(1);
        }
        let w = (rect.right - rect.left).max(0) as u32;
        let h = (rect.bottom - rect.top).max(0) as u32;
        if w == 0 || h == 0 {
            return BOOL(1);
        }

        let title = get_window_title(hwnd).unwrap_or_default();
        let thumbnail = capture_thumbnail(hwnd);

        let hwnd_isize = hwnd.0 as isize;
        eprintln!(
            "[list_windows] hwnd=0x{:X} ({}) rect={}x{} @ ({},{}) title={:?}",
            hwnd_isize, hwnd_isize, w, h, rect.left, rect.top, title
        );

        data.windows.push(WindowInfo {
            hwnd: hwnd_isize,
            title,
            x: rect.left,
            y: rect.top,
            width: w,
            height: h,
            thumbnail,
        });

        BOOL(1) // continue
    }

    pub fn enumerate_windows() -> Result<Vec<WindowInfo>, String> {
        let mut data = EnumWindowsData {
            own_pid: std::process::id(),
            windows: Vec::new(),
        };
        unsafe {
            EnumWindows(
                Some(enum_windows_proc),
                LPARAM(&mut data as *mut EnumWindowsData as isize),
            )
            .map_err(|e| format!("EnumWindows failed: {}", e))?;
        }
        // Sort by title (case-insensitive) so the picker grid has a STABLE
        // ordering across fetches. EnumWindows returns windows in z-order,
        // which changes whenever the user focuses a different window — so
        // re-fetching (e.g. on React StrictMode double-mount or on visibility
        // change) would shuffle the grid. The user could then click the
        // position they originally saw a window in, but that slot now holds
        // a different window. Stable sort prevents this entire class of bug.
        data.windows
            .sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
        Ok(data.windows)
    }
}

// ── Tauri commands ─────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
#[tauri::command]
pub async fn list_monitors() -> Result<Vec<MonitorInfo>, String> {
    // Run blocking Win32 calls on the blocking thread pool
    tokio::task::spawn_blocking(win::enumerate_monitors)
        .await
        .map_err(|e| format!("spawn_blocking failed: {}", e))?
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
pub async fn list_monitors() -> Result<Vec<MonitorInfo>, String> {
    Ok(vec![])
}

#[cfg(target_os = "windows")]
#[tauri::command]
pub async fn list_windows() -> Result<Vec<WindowInfo>, String> {
    tokio::task::spawn_blocking(win::enumerate_windows)
        .await
        .map_err(|e| format!("spawn_blocking failed: {}", e))?
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
pub async fn list_windows() -> Result<Vec<WindowInfo>, String> {
    Ok(vec![])
}

#[cfg(target_os = "windows")]
fn apply_no_window(cmd: &mut Command) {
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
}

#[cfg(not(target_os = "windows"))]
fn apply_no_window(_cmd: &mut Command) {}

fn resolve_ffmpeg(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    crate::commands::recording::resolve_ffmpeg_path(app)
}

#[tauri::command]
pub async fn list_screens() -> Result<Vec<DeviceInfo>, String> {
    Ok(vec![DeviceInfo {
        id: "desktop".to_string(),
        name: "Entire Screen".to_string(),
        device_type: "screen".to_string(),
    }])
}

#[tauri::command]
pub async fn list_audio_devices(app: tauri::AppHandle) -> Result<Vec<DeviceInfo>, String> {
    let ffmpeg_path = match resolve_ffmpeg(&app) {
        Ok(p) => p,
        Err(_) => {
            return Ok(vec![DeviceInfo {
                id: "default".to_string(),
                name: "Default Audio".to_string(),
                device_type: "audio".to_string(),
            }]);
        }
    };

    let mut cmd = Command::new(&ffmpeg_path);
    cmd.args(["-list_devices", "true", "-f", "dshow", "-i", "dummy"]);
    apply_no_window(&mut cmd);

    let output = cmd.output().await.map_err(|e| e.to_string())?;

    // FFmpeg writes device list to stderr (always exits non-zero with dummy input)
    let stderr = String::from_utf8_lossy(&output.stderr);

    let mut devices = Vec::new();

    // Format: [dshow @ ...] "Device Name" (audio)
    //         [dshow @ ...] "Device Name" (video)
    //         [dshow @ ...]   Alternative name "@device_..."
    for line in stderr.lines() {
        // Only look at lines ending with (audio)
        let trimmed = line.trim();
        if !trimmed.ends_with("(audio)") {
            continue;
        }

        // Extract the quoted device name
        if let Some(start) = line.find('"') {
            let after_quote = &line[start + 1..];
            if let Some(end) = after_quote.find('"') {
                let name = &after_quote[..end];
                // Skip @device alternative name lines
                if !name.starts_with("@device") {
                    devices.push(DeviceInfo {
                        id: name.to_string(),
                        name: name.to_string(),
                        device_type: "audio".to_string(),
                    });
                }
            }
        }
    }

    if devices.is_empty() {
        devices.push(DeviceInfo {
            id: "default".to_string(),
            name: "Default Audio".to_string(),
            device_type: "audio".to_string(),
        });
    }

    Ok(devices)
}
