use crate::api_client::{discover, GuiError};
use crate::SharedState;
use planet_port_api::endpoints;
use serde_json::{json, Value};

/// 心跳维护的在线/安装状态（webview 启动初期一次拉取）
#[tauri::command]
pub fn get_daemon_state(state: tauri::State<'_, SharedState>) -> Value {
    let st = state.lock().unwrap();
    json!({ "online": st.online, "installed": st.installed })
}

#[tauri::command]
pub fn get_status() -> Result<Value, GuiError> {
    discover()?.get_json("/status")
}
#[tauri::command]
pub fn get_planets() -> Result<Value, GuiError> {
    discover()?.get_json(&endpoints::planets())
}
#[tauri::command]
pub fn add_planet(source: String) -> Result<Value, GuiError> {
    discover()?.post_json(&endpoints::planets(), &json!({ "source": source }))
}
#[tauri::command]
pub fn remove_planet(id: String) -> Result<Value, GuiError> {
    discover()?.delete(&endpoints::planet(&id))
}
#[tauri::command]
pub fn enable_planet(id: String) -> Result<Value, GuiError> {
    discover()?.post_empty(&endpoints::planet_action(&id, "enable"))
}
#[tauri::command]
pub fn disable_planet(id: String) -> Result<Value, GuiError> {
    discover()?.post_empty(&endpoints::planet_action(&id, "disable"))
}
#[tauri::command]
pub fn get_networks(planet: Option<String>) -> Result<Value, GuiError> {
    discover()?.get_json(&endpoints::networks(planet.as_deref()))
}
#[tauri::command]
pub fn join_network(nwid: String, planet: Option<String>) -> Result<Value, GuiError> {
    discover()?.post_empty(&endpoints::network_join(&nwid, planet.as_deref()))
}
#[tauri::command]
pub fn leave_network(nwid: String, planet: Option<String>) -> Result<Value, GuiError> {
    discover()?.post_empty(&endpoints::network_leave(&nwid, planet.as_deref()))
}
#[tauri::command]
pub fn set_network(nwid: String, planet: Option<String>, body: Value) -> Result<Value, GuiError> {
    discover()?.post_json(&endpoints::network_set(&nwid, planet.as_deref()), &body)
}
#[tauri::command]
pub fn get_peers(planet: Option<String>) -> Result<Value, GuiError> {
    discover()?.get_json(&endpoints::peers(planet.as_deref()))
}
/// 前端语言切换同步托盘菜单（Task 9 消费 lang 字段；此处先落状态）
/// 本任务版本不含 tray 同步（tray 模块 Task 9 才创建）
#[tauri::command]
pub fn set_lang(lang: String, state: tauri::State<'_, SharedState>) -> Result<(), GuiError> {
    state.lock().unwrap().lang = lang;
    Ok(())
}
