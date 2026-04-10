mod commands;
mod migration;
mod state;
pub mod capture_target;

use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Emitter, Manager,
};

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state != tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        return;
                    }
                    // Use exact equality on the lowercased shortcut string. The
                    // previous implementation used `contains("Shift") && contains("S")`
                    // which is buggy because the literal string "Shift" contains the
                    // character 'S' — meaning Ctrl+Shift+D would also fire the
                    // stop-recording handler.
                    let key = shortcut.into_string().to_lowercase();
                    if key == "ctrl+shift+keyd" {
                        // Ctrl+Shift+D: toggle draw mode on overlay
                        if let Some(overlay) = app.get_webview_window("overlay") {
                            let _ = overlay.emit("overlay:toggle-draw-mode", ());
                        }
                    } else if key == "ctrl+shift+keys" {
                        // Ctrl+Shift+S: stop recording
                        if let Some(main_win) = app.get_webview_window("main") {
                            let _ = main_win.emit("recording:stop", ());
                        }
                    }
                })
                .build(),
        )
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::recording::start_recording,
            commands::recording::stop_recording,
            commands::recording::get_recording_state,
            commands::recording::watch_window_rect,
            commands::recording::stop_watching_window,
            commands::recording::switch_capture_target,
            commands::recording::debug_log,
            commands::recordings::list_pending_recordings,
            commands::recordings::get_recording,
            commands::recordings::get_recording_video_path,
            commands::recordings::get_recording_thumbnail_path,
            commands::recordings::delete_recording,
            commands::devices::list_screens,
            commands::devices::list_audio_devices,
            commands::devices::list_monitors,
            commands::devices::list_windows,
            commands::upload::init_upload,
            commands::upload::upload_file,
            commands::auth::get_token,
            commands::auth::save_token,
            commands::auth::clear_token,
            commands::transcription::transcribe_audio,
            commands::frames::extract_keyframes,
            commands::updater::check_for_update,
            commands::setup::test_audio,
            commands::setup::capture_screenshot,
        ])
        .setup(|app| {
            // Bundle-identifier migration: copy settings + orphaned state
            // from com.userfirst.agent (if it exists) to the new app data
            // dir. Safe to run on every startup; no-op when already done.
            migration::run_migration(&app.handle());

            // Recover any orphaned recording from a previous (crashed/killed)
            // Tauri process. If a state file on disk points to an ffmpeg PID
            // that is still alive, we restore it into AppState so the
            // frontend can pick it back up via get_recording_state on mount.
            // This eliminates the "ghost recording" class of bug entirely.
            {
                let app_state: tauri::State<AppState> = app.state();
                commands::recording::recover_orphaned_recording(app_state.inner());
            }
            // Register global shortcuts
            use tauri_plugin_global_shortcut::GlobalShortcutExt;
            let _ = app
                .global_shortcut()
                .register("ctrl+shift+d")
                .map_err(|e| eprintln!("Failed to register Ctrl+Shift+D: {}", e));
            let _ = app
                .global_shortcut()
                .register("ctrl+shift+s")
                .map_err(|e| eprintln!("Failed to register Ctrl+Shift+S: {}", e));

            // Build system tray menu
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let show = MenuItem::with_id(app, "show", "Show Window", true, None::<&str>)?;
            let record = MenuItem::with_id(app, "record", "New Recording", true, None::<&str>)?;
            // "Stop Recording" is always reachable from the tray as a guaranteed
            // escape hatch in case the overlay misbehaves and locks the user out.
            // The frontend listener at RecordingControls.tsx only handles this
            // event while status === 'recording', so it's a no-op otherwise.
            let stop_recording = MenuItem::with_id(app, "stop_recording", "Stop Recording", true, None::<&str>)?;

            let menu = Menu::with_items(app, &[&record, &stop_recording, &show, &quit])?;

            // Auto-grant microphone permission (no browser prompt in desktop app)
            #[cfg(target_os = "windows")]
            {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.with_webview(|webview| {
                        unsafe {
                            use webview2_com::Microsoft::Web::WebView2::Win32::*;
                            use webview2_com::PermissionRequestedEventHandler;

                            if let Ok(core) = webview.controller().CoreWebView2() {
                                let handler = PermissionRequestedEventHandler::create(Box::new(
                                    |_sender, args| {
                                        if let Some(args) = args {
                                            args.SetState(COREWEBVIEW2_PERMISSION_STATE_ALLOW)?;
                                        }
                                        Ok(())
                                    },
                                ));
                                let mut token = std::mem::zeroed();
                                let _ = core.add_PermissionRequested(&handler, &mut token);
                            }
                        }
                    });
                }
            }

            let _tray = TrayIconBuilder::new()
                .menu(&menu)
                .tooltip("Prosper Together")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        app.exit(0);
                    }
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "record" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                            let _ = window.eval("window.location.hash = '#/record'");
                        }
                    }
                    "stop_recording" => {
                        if let Some(main_win) = app.get_webview_window("main") {
                            let _ = main_win.emit("recording:stop", ());
                        }
                    }
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
