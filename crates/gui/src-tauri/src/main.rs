#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api_client;
mod bootstrap;
mod commands;
mod heartbeat;
mod tray;

use std::sync::Mutex;
use tauri::Manager;

/// 全局共享态：探活结论（heartbeat 任务维护，Task 4）+ 托盘语言
pub struct GuiState {
    pub online: bool,
    pub installed: bool,
    pub lang: String,
    pub snapshot_planets: serde_json::Value,
    pub snapshot_networks: serde_json::Value,
}

pub type SharedState = Mutex<GuiState>;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .manage::<SharedState>(Mutex::new(GuiState {
            online: false,
            installed: api_client::installed(),
            lang: String::from("zh"),
            snapshot_planets: serde_json::Value::Array(Vec::new()),
            snapshot_networks: serde_json::Value::Array(Vec::new()),
        }))
        .setup(|app| {
            tray::init(app.handle())?;
            heartbeat::spawn(app.handle().clone());
            // 关窗 = 隐藏到托盘,不退出进程(spec §5.4)
            let win = app.get_webview_window("main").unwrap();
            let w = win.clone();
            win.on_window_event(move |e| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = e {
                    api.prevent_close();
                    let _ = w.hide();
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_daemon_state,
            bootstrap::embedded_manifest,
            bootstrap::start_install,
            commands::get_status,
            commands::get_planets,
            commands::add_planet,
            commands::planet_detail,
            commands::planet_update,
            commands::remove_planet,
            commands::enable_planet,
            commands::disable_planet,
            commands::get_networks,
            commands::join_network,
            commands::leave_network,
            commands::set_network,
            commands::get_peers,
            commands::set_lang,
            commands::planet_export,
            commands::identity_get,
            commands::identity_import_preflight,
            commands::identity_import,
            commands::identity_mode_set,
            commands::write_file_bytes
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
