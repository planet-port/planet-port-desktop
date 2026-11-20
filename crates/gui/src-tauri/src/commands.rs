use crate::api_client::{discover, GuiError};
use crate::SharedState;
use planet_port_api::endpoints;
use serde_json::{json, Value};
use tauri::Manager;

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
/// 前端语言切换同步托盘菜单（菜单文案随 GUI 语言重建）
#[tauri::command]
pub fn set_lang(lang: String, app: tauri::AppHandle) -> Result<(), GuiError> {
    // 块作用域:先释放 SharedState 锁再刷新托盘(tray::refresh 内部也会 lock)
    {
        app.state::<SharedState>().lock().unwrap().lang = lang;
    }
    crate::tray::refresh(&app);
    Ok(())
}
