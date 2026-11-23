use crate::commands;
use crate::SharedState;
use serde_json::Value;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager};

/// 托盘两色四态图标,scripts/make-icons.py 从 256 源图导出:
/// mac/linux 恒用白色单色形状、win 恒用彩色 logo(用户指定口径,不随系统
/// 外观切换);每色两态:online 实心 / offline 同形状淡化(alpha 45%)
#[cfg(target_os = "windows")]
const ICON_ONLINE: &[u8] = include_bytes!("../icons/tray-color-online.png");
#[cfg(target_os = "windows")]
const ICON_OFFLINE: &[u8] = include_bytes!("../icons/tray-color-offline.png");
#[cfg(not(target_os = "windows"))]
const ICON_ONLINE: &[u8] = include_bytes!("../icons/tray-light-online.png");
#[cfg(not(target_os = "windows"))]
const ICON_OFFLINE: &[u8] = include_bytes!("../icons/tray-light-offline.png");

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
        // 非模板:mac 恒白形状不随菜单栏深浅反色(用户指定口径);win 彩色
        // logo 模板模式只对单色有意义
        .icon_as_template(false)
        .tooltip("planet-port")
        .menu(&menu)
        // 左键点托盘 = 打开菜单(网络按行星分组);不开主窗口
        .show_menu_on_left_click(true)
        .on_menu_event(handle_menu_event)
        .build(app)?;
    Ok(())
}

/// 两态图标 + 菜单重建(心跳周期与语言切换时调用)。
/// 门控:托盘菜单开着时被 set_menu/set_icon 替换会立即收起(macOS),
/// 心跳 3s 一拍无条件替换即「菜单很快自动消失」的根因——
/// 故仅在与上次实际设置的内容不同时才替换,数据静稳时菜单可一直开着,
/// 点击菜单外区域由系统原生行为收起
pub fn refresh(app: &AppHandle) {
    let (online, lang, planets, networks, old_fp, old_icon) = {
        let state = app.state::<SharedState>();
        let st = state.lock().unwrap();
        (
            st.online,
            st.lang.clone(),
            st.snapshot_planets.as_array().cloned().unwrap_or_default(),
            st.snapshot_networks.as_array().cloned().unwrap_or_default(),
            st.menu_fp.clone(),
            st.icon_online,
        )
    };
    if let Some(tray) = app.tray_by_id("main") {
        if old_icon != Some(online) {
            let bytes = if online { ICON_ONLINE } else { ICON_OFFLINE };
            if let Ok(img) = tauri::image::Image::from_bytes(bytes) {
                let _ = tray.set_icon(Some(img));
            }
            let _ = tray.set_tooltip(Some(if online { "planet-port: online" } else { "planet-port: offline" }));
            let state = app.state::<SharedState>();
            state.lock().unwrap().icon_online = Some(online);
        }
        let fp = menu_fingerprint(&lang, &planets, &networks);
        if fp != old_fp {
            if let Ok(menu) = build_menu(app) {
                let _ = tray.set_menu(Some(menu));
                let state = app.state::<SharedState>();
                state.lock().unwrap().menu_fp = fp;
            }
        }
    }
}

/// 菜单内容指纹:仅含菜单实际渲染的字段。daemon 快照里的其余字段
/// (peers/uptime 等)每 tick 都变,若纳入则门控形同虚设
fn menu_fingerprint(lang: &str, planets: &[Value], networks: &[Value]) -> String {
    let mut parts = vec![lang.to_string()];
    for p in planets {
        for k in ["id", "name", "status", "nodeId", "type", "coreVersion"] {
            parts.push(p.get(k).map(|v| v.to_string()).unwrap_or_default());
        }
    }
    for n in networks {
        for k in ["planet", "nwid", "name", "status", "type", "conflict", "interface", "assignedAddrs"] {
            parts.push(n.get(k).map(|v| v.to_string()).unwrap_or_default());
        }
    }
    parts.join("\u{1f}")
}

/// 只读信息条目(菜单二级展示用):禁用态灰显,点击无动作,直接挂到子菜单
fn ro(
    app: &AppHandle,
    menu: &Submenu<tauri::Wry>,
    i: &mut usize,
    text: String,
) -> tauri::Result<()> {
    *i += 1;
    menu.append(&MenuItem::with_id(
        app,
        format!("ro|{i}"),
        text,
        false,
        None::<&str>,
    )?)
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
    let mut ro_n = 0usize;
    // 网络按行星分组一级铺开:行星标题行(── 🪐 名 ──,二级 = 行星基本信息只读),
    // 下接该行星网络行(状态点 emoji,二级 = 网络基本信息只读),组尾 + 加入网络…
    for p in &planets {
        let pid = s(p, "id");
        let running = s(p, "status") == "running";
        // 行星标题行:二级 = 基本信息(只读) + 切换到此行星(动作)
        let info = Submenu::with_id(
            app,
            format!("grp|{pid}"),
            format!("── 🪐 {} ──────", s(p, "name")),
            true,
        )?;
        ro(
            app,
            &info,
            &mut ro_n,
            format!(
                "{}: {}",
                tr(&lang, "状态", "Status"),
                if running {
                    tr(&lang, "运行中", "Running")
                } else {
                    tr(&lang, "已停止", "Stopped")
                }
            ),
        )?;
        if !s(p, "nodeId").is_empty() {
            ro(
                app,
                &info,
                &mut ro_n,
                format!("{}: {}", tr(&lang, "节点", "Node"), s(p, "nodeId")),
            )?;
        }
        ro(
            app,
            &info,
            &mut ro_n,
            format!(
                "{}: {}",
                tr(&lang, "类型", "Type"),
                if s(p, "type") == "official" {
                    tr(&lang, "官方", "Official")
                } else {
                    tr(&lang, "自定义", "Custom")
                }
            ),
        )?;
        if !s(p, "coreVersion").is_empty() {
            ro(
                app,
                &info,
                &mut ro_n,
                format!("Core: {}", s(p, "coreVersion")),
            )?;
        }
        info.append(&PredefinedMenuItem::separator(app)?)?;
        // 启停 toggle:运行中→断开(disable),已停→连接(enable),判据同 status
        info.append(&MenuItem::with_id(
            app,
            if running { format!("planetoff|{pid}") } else { format!("planet|{pid}") },
            if running {
                tr(&lang, "断开行星", "Disconnect planet")
            } else {
                tr(&lang, "连接行星", "Connect planet")
            },
            true,
            None::<&str>,
        )?)?;
        menu.append(&info)?;
        // 网络行:二级 = 基本信息(只读) + 离开网络(动作);状态点同前端
        // NetworkCard:conflict 橙/OK 绿/其余白
        for n in networks.iter().filter(|n| s(n, "planet") == pid) {
            let dot = if !s(n, "conflict").is_empty() {
                "🟠"
            } else if s(n, "status") == "OK" {
                "🟢"
            } else {
                "⚪️"
            };
            let name = n.get("name").and_then(|x| x.as_str()).unwrap_or("");
            let nwid = s(n, "nwid");
            let label = if name.is_empty() {
                format!("{dot} {nwid}")
            } else {
                format!("{dot} {name}")
            };
            let net = Submenu::with_id(app, format!("ngrp|{pid}|{nwid}"), label, true)?;
            // 网络ID:首项,可点击复制(id 即复制内容)
            net.append(&MenuItem::with_id(app, format!("copy|{nwid}"), nwid.clone(), true, None::<&str>)?)?;
            if !s(n, "conflict").is_empty() {
                ro(
                    app,
                    &net,
                    &mut ro_n,
                    format!("⚠ {}: {}", tr(&lang, "冲突", "Conflict"), s(n, "conflict")),
                )?;
            }
            ro(
                app,
                &net,
                &mut ro_n,
                format!("{}: {}", tr(&lang, "状态", "Status"), s(n, "status")),
            )?;
            ro(
                app,
                &net,
                &mut ro_n,
                format!("{}: {}", tr(&lang, "类型", "Type"), s(n, "type")),
            )?;
            if !s(n, "interface").is_empty() {
                ro(
                    app,
                    &net,
                    &mut ro_n,
                    format!("{}: {}", tr(&lang, "接口", "Interface"), s(n, "interface")),
                )?;
            }
            // IP 在接口之后,可点击复制(复制纯地址);多地址逐条一行,
            // 首行带 "IP: " 前缀,后续行裸地址
            let addrs: Vec<&str> = n
                .get("assignedAddrs")
                .and_then(|x| x.as_array())
                .map(|v| v.iter().filter_map(|i| i.as_str()).collect())
                .unwrap_or_default();
            for (i, a) in addrs.iter().enumerate() {
                net.append(&MenuItem::with_id(
                    app,
                    format!("copy|{a}"),
                    if i == 0 { format!("IP: {a}") } else { (*a).to_string() },
                    true,
                    None::<&str>,
                )?)?;
            }
            net.append(&PredefinedMenuItem::separator(app)?)?;
            // 启停 toggle:已连接(OK)→断开(disable),其余→连接(enable),判据同前端 NetworkCard
            let on = s(n, "status") == "OK";
            net.append(&MenuItem::with_id(
                app,
                if on { format!("netoff|{pid}|{nwid}") } else { format!("neton|{pid}|{nwid}") },
                if on {
                    tr(&lang, "断开网络", "Disconnect network")
                } else {
                    tr(&lang, "连接网络", "Connect network")
                },
                true,
                None::<&str>,
            )?)?;
            net.append(&MenuItem::with_id(
                app,
                format!("net|{pid}|{nwid}"),
                tr(&lang, "退出网络", "Leave network"),
                true,
                None::<&str>,
            )?)?;
            menu.append(&net)?;
        }
        // 加入网络(带行星维度,打开主窗口聚焦输入框)
        menu.append(&MenuItem::with_id(
            app,
            format!("join|{pid}"),
            tr(&lang, "+ 加入网络…", "+ Join network…"),
            true,
            None::<&str>,
        )?)?;
        menu.append(&PredefinedMenuItem::separator(app)?)?;
    }
    menu.append(&MenuItem::with_id(
        app,
        "open",
        tr(&lang, "主窗口", "Open window"),
        true,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(
        app,
        "addplanet",
        tr(&lang, "+ 加入行星", "+ Add planet"),
        true,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(
        app,
        "quit",
        tr(&lang, "退出", "Quit"),
        true,
        None::<&str>,
    )?)?;
    Ok(menu)
}

fn handle_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    let id = event.id().as_ref().to_string();
    let parts: Vec<&str> = id.split('|').collect();
    match parts[0] {
        "net" if parts.len() == 3 => {
            // 网络子菜单「退出网络」(leave);planet 维度随条目携带。
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
        // 网络启停 toggle:连接(enable) / 断开(disable),HTTP 动作同走后台线程
        "neton" | "netoff" if parts.len() == 3 => {
            let enable = parts[0] == "neton";
            let app = app.clone();
            let nwid = parts[2].to_string();
            let planet = parts[1].to_string();
            std::thread::spawn(move || {
                let r = if enable {
                    commands::enable_network(nwid, Some(planet))
                } else {
                    commands::disable_network(nwid, Some(planet))
                };
                if let Err(e) = r {
                    notify_error(&app, &e);
                }
                refresh(&app);
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
        // 行星断开(disable);连接复用 "planet"(enable)
        "planetoff" => {
            let app = app.clone();
            let planet = parts[1].to_string();
            std::thread::spawn(move || {
                if let Err(e) = commands::disable_planet(planet) {
                    notify_error(&app, &e);
                }
                refresh(&app);
            });
        }
        // 复制到剪贴板(网络 ID / IP 条目);id 后半段即复制内容
        "copy" => {
            use tauri_plugin_clipboard_manager::ClipboardExt;
            let _ = app.clipboard().write_text(parts[1..].join("|"));
        }
        "join" => {
            // 带行星维度:加入网络抽屉预选该行星(parts = ["join", planet])
            let planet = parts.get(1).map(|p| p.to_string());
            let _ = app.emit(
                "navigate",
                serde_json::json!({ "to": "home", "focusJoin": true, "planet": planet }),
            );
            show_main(app);
        }
        "addplanet" => {
            let _ = app.emit(
                "navigate",
                serde_json::json!({ "to": "home", "addPlanet": true }),
            );
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

/// 拉起主窗口:先恢复 Dock 图标(关窗时藏了)再 show+focus。
/// single_instance 二次启动拉活也走这里
pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        // 仅 macOS 有 Dock;Windows/Linux 为 no-op 不受影响
        #[cfg(target_os = "macos")]
        let _ = app.set_dock_visibility(true);
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::{menu_fingerprint, tr};
    use serde_json::json;

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

    /// 菜单渲染字段变化 → 指纹变化(refresh 触发 set_menu)
    #[test]
    fn fingerprint_changes_when_menu_fields_change() {
        let p1 = vec![json!({"id": "1", "name": "gozi", "status": "running"})];
        let p2 = vec![json!({"id": "1", "name": "gozi", "status": "stopped"})];
        assert_ne!(
            menu_fingerprint("zh", &p1, &[]),
            menu_fingerprint("zh", &p2, &[])
        );
        // 语言切换也触发(菜单文案重建)
        assert_ne!(
            menu_fingerprint("zh", &p1, &[]),
            menu_fingerprint("en", &p1, &[])
        );
    }

    /// 菜单不渲染的字段变化 → 指纹不变(refresh 跳过 set_menu,
    /// 托盘菜单开着时不被顶掉)
    #[test]
    fn fingerprint_stable_for_non_menu_fields() {
        let a = vec![json!({"id": "1", "name": "gozi", "status": "running", "uptime": 10, "peers": 99})];
        let b = vec![json!({"id": "1", "name": "gozi", "status": "running", "uptime": 20, "peers": 0})];
        assert_eq!(
            menu_fingerprint("zh", &a, &[json!({"nwid": "a", "latency": 5})]),
            menu_fingerprint("zh", &b, &[json!({"nwid": "a", "latency": 120})])
        );
    }
}
