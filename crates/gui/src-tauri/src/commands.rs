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
    discover()?.get_json(endpoints::status())
}
#[tauri::command]
pub fn get_planets() -> Result<Value, GuiError> {
    discover()?.get_json(&endpoints::planets())
}
#[tauri::command]
pub fn add_planet(
    source: String,
    name: Option<String>,
    icon: Option<String>,
    color: Option<String>,
) -> Result<Value, GuiError> {
    // POST /planets 既有 body 面:source 必填,name/icon/color 可选
    // (daemon 已支持,GUI 仅透传;URL 源=source 含 ://)
    let mut body = json!({ "source": source });
    if let Some(v) = name {
        body["name"] = Value::from(v);
    }
    if let Some(v) = icon {
        body["icon"] = Value::from(v);
    }
    if let Some(v) = color {
        body["color"] = Value::from(v);
    }
    discover()?.post_json(&endpoints::planets(), &body)
}
/// 行星详情(GET /planets/{id}):WorldID/文件大小/修订时间等只读信息
#[tauri::command]
pub fn planet_detail(id: String) -> Result<Value, GuiError> {
    discover()?.get_json(&endpoints::planet(&id))
}
/// 行星改名/图标/底色(PATCH /planets/{id};daemon 已支持,GUI 仅透传)
#[tauri::command]
pub fn planet_update(
    id: String,
    name: Option<String>,
    icon: Option<String>,
    color: Option<String>,
) -> Result<Value, GuiError> {
    let mut body = json!({});
    if let Some(v) = name {
        body["name"] = Value::from(v);
    }
    if let Some(v) = icon {
        body["icon"] = Value::from(v);
    }
    if let Some(v) = color {
        body["color"] = Value::from(v);
    }
    discover()?.patch_json(&endpoints::planet(&id), &body)
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

/// planet 二进制导出(daemon octet-stream 原样字节;前端配合保存对话框落盘)
#[tauri::command]
pub fn planet_export(id: String) -> Result<Vec<u8>, GuiError> {
    discover()?.get_bytes(&endpoints::planet_export(&id))
}
/// 节点身份信息:全局身份 + 各行星实际身份
#[tauri::command]
pub fn identity_get() -> Result<Value, GuiError> {
    discover()?.get_json(endpoints::identity())
}
/// 身份导入预检(只读):body 带 preflight 标记,旧 daemon 对独立路由 404
#[tauri::command]
pub fn identity_import_preflight(path: String) -> Result<Value, GuiError> {
    discover()?.post_json(&endpoints::identity_import_preflight(), &json!({ "preflight": true, "path": path }))
}
/// 身份导入:GUI 固定 shared 模式(无模式切换 UI);force 由前端覆盖确认后置真
#[tauri::command]
pub fn identity_import(path: String, force: bool) -> Result<Value, GuiError> {
    discover()?.post_json(&endpoints::identity_import(), &json!({ "path": path, "force": force, "mode": "shared" }))
}
/// 前端数据(dataURL 解码后/atob 后的 bytes)写盘:二维码 PNG、planet 导出共用
#[tauri::command]
pub fn write_file_bytes(path: String, bytes: Vec<u8>) -> Result<(), GuiError> {
    std::fs::write(&path, bytes).map_err(|e| GuiError::Usage { message: format!("write {path}: {e}") })
}
