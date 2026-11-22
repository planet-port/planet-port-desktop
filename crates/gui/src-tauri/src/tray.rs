use crate::commands;
use crate::SharedState;
use serde_json::Value;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

/// 占位图标由 scripts/make-icons.py 生成(圆环风格)
const ICON_ONLINE: &[u8] = include_bytes!("../icons/tray-online.png");
const ICON_OFFLINE: &[u8] = include_bytes!("../icons/tray-offline.png");

/// 托盘菜单文案(Rust 侧两份,与前端 i18n 键语义一致;set_lang 切换后重建菜单)。
/// lang 由调用方在持锁期间一次性取出传入——避免 tr 内部二次 lock 同一把 Mutex 死锁
fn tr(lang: &str, zh: &str, en: &str) -> String {
    if lang == "en" { en.to_string() } else { zh.to_string() }
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app)?;
    TrayIconBuilder::with_id("main")
        .icon(tauri::image::Image::from_bytes(ICON_OFFLINE)?)
        .tooltip("planet-port")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(handle_menu_event)
        .on_tray_icon_event(|tray, event| {
            // 左键点托盘 = 开/聚焦主窗口(spec §5.4)
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// 两态图标 + 菜单重建(心跳周期与语言切换时调用)
pub fn refresh(app: &AppHandle) {
    let online = app.state::<SharedState>().lock().unwrap().online;
    if let Some(tray) = app.tray_by_id("main") {
        let bytes = if online { ICON_ONLINE } else { ICON_OFFLINE };
        if let Ok(img) = tauri::image::Image::from_bytes(bytes) {
            let _ = tray.set_icon(Some(img));
        }
        let _ = tray.set_tooltip(Some(if online { "planet-port: online" } else { "planet-port: offline" }));
        if let Ok(menu) = build_menu(app) {
            let _ = tray.set_menu(Some(menu));
        }
    }
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    // 锁纪律:持锁期间只取数据不回调。菜单条目构造内部会 dispatch 到主线程并阻塞等返回,
    // 若持锁构建,主线程上的同步 command(get_daemon_state 等)恰需同一把锁时即互等死锁。
    let (lang, networks, planets) = {
        let state = app.state::<SharedState>();
        let st = state.lock().unwrap();
        (
            st.lang.clone(),
            st.snapshot_networks.as_array().cloned().unwrap_or_default(),
            st.snapshot_planets.as_array().cloned().unwrap_or_default(),
        )
    };
    let menu = Menu::new(app)?;
    // 快速操作①:已 join 网络开关(点击 = leave;加入走「加入网络…」)
    for n in &networks {
        let name = n.get("name").and_then(|x| x.as_str()).unwrap_or("");
        let label = if name.is_empty() { s(n, "nwid") } else { format!("{}  {}", s(n, "nwid"), name) };
        let item = CheckMenuItem::with_id(app, format!("net|{}|{}", s(n, "planet"), s(n, "nwid")), label, true, true, None::<&str>)?;
        menu.append(&item)?;
    }
    // 快速操作②:加入网络(打开主窗口聚焦输入框)
    menu.append(&MenuItem::with_id(app, "join", tr(&lang, "+ 加入网络…", "+ Join network…"), true, None::<&str>)?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    // 快速操作③:planet 单选切换(●=当前运行)
    for p in &planets {
        let active = s(p, "status") == "running";
        let label = format!("{} {}", if active { "●" } else { "○" }, s(p, "name"));
        menu.append(&MenuItem::with_id(app, format!("planet|{}", s(p, "id")), label, true, None::<&str>)?)?;
    }
    if !planets.is_empty() {
        menu.append(&PredefinedMenuItem::separator(app)?)?;
    }
    menu.append(&MenuItem::with_id(app, "open", tr(&lang, "打开主窗口", "Open window"), true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "quit", tr(&lang, "退出", "Quit"), true, None::<&str>)?)?;
    Ok(menu)
}

fn handle_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    let id = event.id().as_ref().to_string();
    let parts: Vec<&str> = id.split('|').collect();
    match parts[0] {
        "net" if parts.len() == 3 => {
            // 点击已勾选网络 = leave;planet 维度随条目携带。
            // 动作含同步 HTTP(超时 10s),必须在独立线程执行,否则冻结主线程/菜单;
            // 结果(图标+菜单)与失败提示分别经 refresh 与 tray-error 事件回传
            let app = app.clone();
            let nwid = parts[2].to_string();
            let planet = parts[1].to_string();
            std::thread::spawn(move || {
                if let Err(e) = commands::leave_network(nwid, Some(planet)) {
                    notify_error(&app, &e);
                }
                refresh(&app); // 立即反映;下次心跳会再校准
            });
        }
        "planet" => {
            let app = app.clone();
            let planet = parts[1].to_string();
            std::thread::spawn(move || {
                if let Err(e) = commands::enable_planet(planet) {
                    notify_error(&app, &e);
                }
                refresh(&app);
            });
        }
        "join" => {
            let _ = app.emit("navigate", serde_json::json!({ "to": "home", "focusJoin": true }));
            show_main(app);
        }
        "open" => show_main(app),
        "quit" => app.exit(0),
        _ => {}
    }
}

/// 托盘动作失败回传前端 toast(GuiError 各变体均带 message)
fn notify_error(app: &AppHandle, e: &crate::api_client::GuiError) {
    let message = match e {
        crate::api_client::GuiError::Offline { message }
        | crate::api_client::GuiError::Usage { message }
        | crate::api_client::GuiError::Api { message, .. } => message.clone(),
    };
    let _ = app.emit("tray-error", message);
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::tr;

    /// 默认（zh 及任何未知值）取中文文案
    #[test]
    fn tr_returns_zh_for_zh() {
        assert_eq!(tr("zh", "打开主窗口", "Open window"), "打开主窗口");
        assert_eq!(tr("fr", "退出", "Quit"), "退出");
    }

    /// 仅显式 "en" 切英文（set_lang 切换后重建菜单走此分支）
    #[test]
    fn tr_returns_en_for_en() {
        assert_eq!(tr("en", "打开主窗口", "Open window"), "Open window");
        assert_eq!(tr("en", "退出", "Quit"), "Quit");
    }
}
