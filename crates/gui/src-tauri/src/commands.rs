use crate::api_client::{discover, GuiError};
use crate::SharedState;
use planet_port_api::endpoints;
use serde_json::{json, Value};
use std::time::Duration;
use tauri::Manager;

/// GitHub releases 列表拉取超时(与 api_client 常规请求同口径)
const GITHUB_TIMEOUT: Duration = Duration::from_secs(10);

/// GitHub Releases 里 GUI 最新稳定版号(剥 planet-port-gui/v 前缀)。
/// releases/latest 是全仓混合口径,组件 tag 必须列表过滤;prerelease
/// 过滤与 install.sh auto-latest 同口径
pub(crate) fn parse_latest_gui_release(body: &str) -> Option<String> {
    let arr: Vec<serde_json::Value> = serde_json::from_str(body).ok()?;
    arr.iter()
        .filter(|r| r.get("prerelease").and_then(|v| v.as_bool()) == Some(false))
        .find_map(|r| {
            let tag = r.get("tag_name")?.as_str()?;
            tag.strip_prefix("planet-port-gui/v").map(str::to_string)
        })
}

/// 手动触发的 GUI 更新检查:ureq 走 api.github.com(gh 同源路径,本机
/// 网络可达性最好);超时 10s,失败如实回传中文错误,前端弹窗呈现。
/// 请求构造贴 api_client.rs 的 http::Request + agent.run 惯用法;
/// (async) 派生独立线程——公网拉取最长 10s,同步默认跑主线程会冻 UI
#[tauri::command(async)]
pub fn check_gui_update() -> Result<String, String> {
    const URL: &str =
        "https://api.github.com/repos/planet-port/planet-port-desktop/releases?per_page=30";
    // http_status_as_error(false):限流/不存在等非 2xx 仍拿到响应体,
    // 由下方状态码分支转成带码位的中文错误,而非英文 http status 文案
    let agent = ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .http_status_as_error(false)
            .timeout_global(Some(GITHUB_TIMEOUT))
            .build(),
    );
    let req = ureq::http::Request::builder()
        .method("GET")
        .uri(URL)
        .header("User-Agent", "planet-port-gui")
        .header("Accept", "application/vnd.github+json")
        .body(Vec::new())
        .map_err(|e| format!("构造请求失败: {e}"))?;
    let mut resp = agent.run(req).map_err(|e| format!("网络请求失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("GitHub 响应异常: HTTP {}", resp.status().as_u16()));
    }
    let body = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("读取响应失败: {e}"))?;
    parse_latest_gui_release(&body).ok_or_else(|| String::from("未找到稳定版发布"))
}

/// 心跳维护的在线/安装状态（webview 启动初期一次拉取）
#[tauri::command]
pub fn get_daemon_state(state: tauri::State<'_, SharedState>) -> Value {
    let st = state.lock().unwrap();
    json!({
        "online": st.online,
        "installed": st.installed,
        "daemonVersion": st.daemon_version,
    })
}

/// 关于区展示用:已装 cli 版本(未装返回 None,前端回退内嵌 manifest.cli)
#[tauri::command]
pub fn get_cli_version() -> Option<String> {
    crate::api_client::installed_cli_version()
}

/// 已装 daemon 版本(spawn 探测,离线可用):版本差异检测比对的是磁盘
/// 二进制,与心跳 /version(运行中版本)口径不同——重装覆盖磁盘、旧进程
/// 未重启时两者会短暂不一致
#[tauri::command]
pub fn get_installed_daemon_version() -> Option<String> {
    crate::api_client::installed_daemon_version()
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
pub fn connect_network(nwid: String, planet: Option<String>) -> Result<Value, GuiError> {
    discover()?.post_empty(&endpoints::network_connect(&nwid, planet.as_deref()))
}
#[tauri::command]
pub fn disconnect_network(nwid: String, planet: Option<String>) -> Result<Value, GuiError> {
    discover()?.post_empty(&endpoints::network_disconnect(&nwid, planet.as_deref()))
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
/// 身份导入:body.mode 缺省,daemon 保持现有模式不擅改——模式管理走 identity_mode_set;
/// force 由前端覆盖确认后置真
#[tauri::command]
pub fn identity_import(path: String, force: bool) -> Result<Value, GuiError> {
    discover()?.post_json(&endpoints::identity_import(), &json!({ "path": path, "force": force }))
}
/// 身份模式切换(PUT /identity):行星运行中 daemon 409 拒绝,前端预检+确认
#[tauri::command]
pub fn identity_mode_set(mode: String) -> Result<Value, GuiError> {
    discover()?.put_json(endpoints::identity(), &json!({ "mode": mode }))
}
/// 前端数据(dataURL 解码后/atob 后的 bytes)写盘:二维码 PNG、planet 导出共用
#[tauri::command]
pub fn write_file_bytes(path: String, bytes: Vec<u8>) -> Result<(), GuiError> {
    std::fs::write(&path, bytes).map_err(|e| GuiError::Usage { message: format!("write {path}: {e}") })
}

/// 应用背景图:复制用户所选图片进 appData/background/current.<ext> 并返回
/// 目标绝对路径(前端 convertFileSrc 经 asset 协议引用)。拷贝而非记原路径
/// ——原文件被移动/删除后背景会随之失效;目录内旧文件先清,避免残留堆积
#[tauri::command]
pub fn background_set(app: tauri::AppHandle, path: String) -> Result<String, GuiError> {
    let dir = app.path().app_data_dir()
        .map_err(|e| GuiError::Usage { message: format!("app_data_dir: {e}") })?
        .join("background");
    let src = std::path::Path::new(&path);
    let ext = src.extension().and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_else(|| "png".to_string());
    clear_background_dir(&dir);
    std::fs::create_dir_all(&dir)
        .map_err(|e| GuiError::Usage { message: format!("mkdir {}: {e}", dir.display()) })?;
    let dest = dir.join(format!("current.{ext}"));
    std::fs::copy(src, &dest)
        .map_err(|e| GuiError::Usage { message: format!("copy to {}: {e}", dest.display()) })?;
    dest.to_str().map(String::from)
        .ok_or_else(|| GuiError::Usage { message: "background path not utf-8".into() })
}

/// 清除背景图:删除 appData/background 下全部文件(路径记录由前端清)
#[tauri::command]
pub fn background_clear(app: tauri::AppHandle) -> Result<(), GuiError> {
    let dir = app.path().app_data_dir()
        .map_err(|e| GuiError::Usage { message: format!("app_data_dir: {e}") })?
        .join("background");
    clear_background_dir(&dir);
    Ok(())
}

/// 目录存在则清空其下常规文件(扩展名可能变化,统一按目录清)
fn clear_background_dir(dir: &std::path::Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// ZeroTier 官方 planet 二进制(570B):与官方客户端内置默认 world 逐字节
/// 一致,提取自 vendor 的 ZeroTier 源码 node/Topology.cpp 的 ZT_DEFAULT_WORLD
const OFFICIAL_WORLD: &[u8] = include_bytes!("../assets/official-planet.bin");

/// 一键添加官方行星:内置官方 world 写入 appData 后走常规 add_planet;
/// worldId=149604618 由 daemon 识别自动标记 Official。icon/color 由前端
/// 传入(zerotier 专用图标 + 品牌黑),名称缺省 ZeroTier
#[tauri::command]
pub fn add_official_planet(
    app: tauri::AppHandle,
    icon: Option<String>,
    color: Option<String>,
) -> Result<Value, GuiError> {
    let dir = app.path().app_data_dir()
        .map_err(|e| GuiError::Usage { message: format!("app_data_dir: {e}") })?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| GuiError::Usage { message: format!("mkdir {}: {e}", dir.display()) })?;
    let path = dir.join("official-planet.bin");
    std::fs::write(&path, OFFICIAL_WORLD)
        .map_err(|e| GuiError::Usage { message: format!("write {}: {e}", path.display()) })?;
    add_planet(path.to_string_lossy().into_owned(), Some("ZeroTier".into()), icon, color)
}

#[cfg(test)]
mod tests {
    use super::parse_latest_gui_release;

    #[test]
    fn parse_latest_filters_prefix_and_prerelease() {
        let body = r#"[
          {"tag_name":"planet-port/v0.6.0","prerelease":false},
          {"tag_name":"planet-port-gui/v0.6.0-beta.1","prerelease":true},
          {"tag_name":"planet-portd/v0.5.1","prerelease":false},
          {"tag_name":"planet-port-gui/v0.6.0","prerelease":false}
        ]"#;
        assert_eq!(parse_latest_gui_release(body).as_deref(), Some("0.6.0"));
    }

    #[test]
    fn parse_latest_takes_first_match_in_order() {
        let body = r#"[
          {"tag_name":"planet-port-gui/v0.7.0","prerelease":false},
          {"tag_name":"planet-port-gui/v0.6.0","prerelease":false}
        ]"#;
        assert_eq!(parse_latest_gui_release(body).as_deref(), Some("0.7.0"));
    }

    #[test]
    fn parse_latest_none_when_no_stable_gui() {
        let body = r#"[{"tag_name":"planet-port-gui/v0.7.0-beta.1","prerelease":true}]"#;
        assert_eq!(parse_latest_gui_release(body), None);
    }

    #[test]
    fn parse_latest_none_on_garbage() {
        assert_eq!(parse_latest_gui_release("not json"), None);
        assert_eq!(parse_latest_gui_release("[]"), None);
    }
}
