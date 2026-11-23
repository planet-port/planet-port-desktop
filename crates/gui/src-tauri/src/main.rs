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
    /// 上次 set_menu 时的菜单内容指纹(refresh 门控用,见 tray::refresh)
    pub menu_fp: String,
    /// 上次 set_icon 时的在线态(None = 尚未设置过)
    pub icon_online: Option<bool>,
}

pub type SharedState = Mutex<GuiState>;

/// 构建时间(build.rs 注入;dev 增量编译沿用上次,打包环境必新鲜)
#[tauri::command]
fn build_time() -> String {
    env!("PP_BUILD_TIME").to_string()
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage::<SharedState>(Mutex::new(GuiState {
            online: false,
            installed: api_client::installed(),
            lang: String::from("zh"),
            snapshot_planets: serde_json::Value::Array(Vec::new()),
            snapshot_networks: serde_json::Value::Array(Vec::new()),
            menu_fp: String::new(),
            icon_online: None,
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
                    // 关窗后藏 Dock 图标(仅 macOS):应用退居托盘,Dock 不再占位;
                    // show_main 拉起窗口时恢复。Windows/Linux 无此概念不受影响
                    #[cfg(target_os = "macos")]
                    let _ = w.app_handle().set_dock_visibility(false);
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
            commands::enable_network,
            commands::disable_network,
            commands::set_network,
            commands::get_peers,
            commands::set_lang,
            commands::planet_export,
            commands::identity_get,
            commands::identity_import_preflight,
            commands::identity_import,
            commands::identity_mode_set,
            commands::write_file_bytes,
            commands::background_set,
            commands::background_clear,
            commands::add_official_planet,
            build_time
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
