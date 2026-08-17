//! `pimp-dsh-desktop` — Windows-first Tauri 2 desktop supervisor.
//!
//! Rust owns the lifecycle; React is a view. The reviewed `unsafe` Win32 calls
//! are isolated in `platform/` behind explicit `unsafe {}` blocks.

#![deny(unsafe_op_in_unsafe_fn)]

pub mod commands;
pub mod compatibility;
pub mod logging;
pub mod manifest;
pub mod platform;
pub mod protocol;
pub mod state;
pub mod supervisor;
pub mod types;

// Re-exports matching the cross-slice (DesktopTests) public surface.
pub mod job {
    pub use crate::platform::job::*;
}
pub mod pipe {
    pub use crate::platform::pipe::*;
}
pub mod provider {
    pub use crate::compatibility::*;
}
pub mod logs {
    pub use crate::logging::*;
}

#[cfg(not(test))]
mod desktop_app {
    use super::*;

    use std::time::Duration;

    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    use tauri::{Emitter, Manager, WindowEvent};

    use crate::state::State;

    // ---- Tauri command handlers (delegate to the plain `commands` surface) ----

    #[tauri::command]
    fn get_snapshot() -> types::Snapshot {
        commands::get_snapshot()
    }

    #[tauri::command]
    fn start_harness() -> Result<(), String> {
        commands::start_harness()
    }

    #[tauri::command]
    fn stop_harness() -> Result<(), String> {
        commands::stop_harness()
    }

    #[tauri::command]
    fn run_doctor() -> Result<(), String> {
        commands::run_doctor()
    }

    #[tauri::command]
    fn open_harness(app: tauri::AppHandle) -> Result<(), String> {
        let url = commands::validated_endpoint()?;
        open_harness_window(&app, &url)
    }

    #[tauri::command]
    fn reveal_log_folder() -> Result<(), String> {
        commands::reveal_log_folder()
    }

    #[tauri::command]
    fn set_theme(theme: types::Theme) -> Result<(), String> {
        commands::set_theme(theme)
    }

    #[tauri::command]
    fn set_fixed_port(port: Option<u16>) -> Result<(), String> {
        commands::set_fixed_port(port)
    }

    fn focus_main(app: &tauri::AppHandle) {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.show();
            let _ = w.set_focus();
        }
    }

    fn open_harness_window(app: &tauri::AppHandle, url: &str) -> Result<(), String> {
        let url =
            tauri::Url::parse(url).map_err(|e| format!("invalid endpoint: {e}"))?;
        let window = app
            .get_webview_window("harness")
            .ok_or_else(|| "harness window not found".to_string())?;
        window
            .navigate(url)
            .map_err(|e| format!("navigate harness window: {e}"))?;
        let _ = window.show();
        let _ = window.set_focus();
        Ok(())
    }

    pub fn run() {
        tauri::Builder::default()
            // Must be the first plugin so duplicate launches cannot initialize
            // another controller before they are redirected to this instance.
            .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
                focus_main(app);
            }))
            .setup(|app| {
                let supervisor = commands::init_supervisor();
                let resource_dir = app.path().resource_dir().ok();
                let emitter = app.handle().clone();
                let auto_opened = std::sync::Arc::new(std::sync::Mutex::new(None::<u64>));
                supervisor.set_emitter(resource_dir, {
                    let emitter = emitter.clone();
                    let auto_opened = auto_opened.clone();
                    move |snapshot| {
                        // Product-first: on reaching Running, bring the harness
                        // webview forward and demote the lobby to the tray. Guard
                        // on revision so this fires once per run, not on every
                        // Running snapshot (logs/health re-emit without a
                        // transition).
                        if snapshot.state == State::Running {
                            let mut last = auto_opened.lock().expect("auto-open lock");
                            if *last != Some(snapshot.revision) {
                                *last = Some(snapshot.revision);
                                if let Ok(url) = commands::validated_endpoint() {
                                    let _ = open_harness_window(&emitter, &url);
                                    if let Some(main) = emitter.get_webview_window("main") {
                                        let _ = main.hide();
                                    }
                                }
                            }
                        }
                        let _ = emitter.emit("supervisor://snapshot", snapshot);
                    }
                });

                build_tray(app)?;
                Ok(())
            })
            .on_window_event(|window, event| {
                // Close-to-tray for both windows: the supervisor stays resident.
                // The product window hides (never destroys) so "Open Web UI"
                // always re-navigates and un-hides it.
                if let WindowEvent::CloseRequested { api, .. } = event {
                    let _ = window.hide();
                    api.prevent_close();
                }
            })
            .invoke_handler(tauri::generate_handler![
                get_snapshot,
                start_harness,
                stop_harness,
                run_doctor,
                open_harness,
                reveal_log_folder,
                set_theme,
                set_fixed_port
            ])
            .build(tauri::generate_context!())
            .expect("error while building the tauri application")
            .run(|app_handle, event| {
                if let tauri::RunEvent::ExitRequested { api, .. } = event {
                    // Quit follows the stop policy: never silently detach a run.
                    let snap = commands::get_snapshot();
                    let active = matches!(
                        snap.state,
                        State::Preflighting
                            | State::Starting
                            | State::Ready
                            | State::Running
                            | State::Stopping
                    );
                    if active {
                        api.prevent_exit();
                        let _ = commands::stop_harness();
                        let handle = app_handle.clone();
                        std::thread::spawn(move || {
                            loop {
                                let s = commands::get_snapshot();
                                if !matches!(
                                    s.state,
                                    State::Preflighting
                                        | State::Starting
                                        | State::Ready
                                        | State::Running
                                        | State::Stopping
                                ) {
                                    handle.exit(0);
                                    return;
                                }
                                std::thread::sleep(Duration::from_millis(100));
                            }
                        });
                    }
                }
            });
    }

    fn build_tray(app: &tauri::App) -> tauri::Result<()> {
        let show = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
        let start = MenuItem::with_id(app, "start", "Start", true, None::<&str>)?;
        let stop = MenuItem::with_id(app, "stop", "Stop", true, None::<&str>)?;
        let open = MenuItem::with_id(app, "open", "Open Web UI", true, None::<&str>)?;
        let reveal = MenuItem::with_id(app, "reveal", "Reveal Logs", true, None::<&str>)?;
        let doctor = MenuItem::with_id(app, "doctor", "Run diagnostics", true, None::<&str>)?;
        let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
        let menu = Menu::with_items(app, &[&show, &start, &stop, &open, &doctor, &reveal, &quit])?;

        let icon = app
            .default_window_icon()
            .cloned()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no window icon"))?;

        let _tray = TrayIconBuilder::new()
            .icon(icon)
            .menu(&menu)
            .show_menu_on_left_click(false)
            .on_menu_event(|app, event| match event.id.as_ref() {
                "show" => focus_main(app),
                "start" => {
                    let _ = commands::start_harness();
                }
                "stop" => {
                    let _ = commands::stop_harness();
                }
                "open" => {
                    let _ = commands::validated_endpoint()
                        .and_then(|url| open_harness_window(app, &url));
                }
                "reveal" => {
                    let _ = commands::reveal_log_folder();
                }
                "doctor" => {
                    let _ = commands::run_doctor();
                }
                "quit" => app.exit(0),
                _ => {}
            })
            .on_tray_icon_event(|tray, event| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    focus_main(tray.app_handle());
                }
            })
            .build(app)?;

        Ok(())
    }
}

#[cfg(not(test))]
pub use desktop_app::run;
