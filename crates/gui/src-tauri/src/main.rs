#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api_client;
mod bootstrap;
mod commands;
mod heartbeat;
mod tray;

use std::sync::Mutex;

/// 全局共享态：探活结论（heartbeat 任务维护，Task 4）+ 托盘语言
pub struct GuiState {
    pub online: bool,
    pub installed: bool,
    /// 已装/运行中 daemon 的版本(heartbeat 维护):在线取 /version,离线读
    /// daemon bin --version 一次缓存;None = 不可得(未装/读取失败)
    pub daemon_version: Option<String>,
    pub lang: String,
    pub snapshot_planets: serde_json::Value,
    pub snapshot_networks: serde_json::Value,
    /// 上次 set_menu 时的菜单内容指纹(refresh 门控用,见 tray::refresh)
    pub menu_fp: String,
    /// 上次 set_icon 时的在线态(None = 尚未设置过)
    pub icon_online: Option<bool>,
}

pub type SharedState = Mutex<GuiState>;

/// 构建时间(build.rs 注入 epoch,显示期转查看者本地时区;
/// dev 增量编译沿用上次,打包环境必新鲜)
#[tauri::command]
fn build_time() -> String {
    option_env!("PP_BUILD_EPOCH")
        .and_then(|s| s.parse::<i64>().ok())
        .and_then(|secs| chrono::DateTime::from_timestamp(secs, 0))
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .manage::<SharedState>(Mutex::new(GuiState {
            online: false,
            installed: api_client::installed(),
            daemon_version: None,
            lang: String::from("zh"),
            snapshot_planets: serde_json::Value::Array(Vec::new()),
            snapshot_networks: serde_json::Value::Array(Vec::new()),
            menu_fp: String::new(),
            icon_online: None,
        }))
        .setup(|app| {
            tray::init(app.handle())?;
            heartbeat::spawn(app.handle().clone());
            // 关窗即退出:不拦截 CloseRequested,走 tauri 默认「最后一个
            // 窗口关闭即退出进程」(托盘随之消失);窗口异常关闭而进程仍
            // 存活的场景由下方 RunEvent::Reopen 兜底拉回
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_daemon_state,
            commands::get_cli_version,
            commands::get_installed_daemon_version,
            commands::check_gui_update,
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
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // macOS 点 Dock 图标:tao 委托在无可见窗口时返回 NO(系统零动作),
            // 窗口恢复完全仰赖此事件——异常关窗进程仍存活时点图标可拉回
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                tray::show_main(app);
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
}
