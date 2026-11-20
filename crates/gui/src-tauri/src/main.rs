#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api_client;
mod commands;
mod heartbeat;

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
        .manage::<SharedState>(Mutex::new(GuiState {
            online: false,
            installed: api_client::installed(),
            lang: String::from("zh"),
            snapshot_planets: serde_json::Value::Array(Vec::new()),
            snapshot_networks: serde_json::Value::Array(Vec::new()),
        }))
        .setup(|app| {
            heartbeat::spawn(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_daemon_state,
            commands::get_status,
            commands::get_planets,
            commands::add_planet,
            commands::remove_planet,
            commands::enable_planet,
            commands::disable_planet,
            commands::get_networks,
            commands::join_network,
            commands::leave_network,
            commands::set_network,
            commands::get_peers,
            commands::set_lang
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
