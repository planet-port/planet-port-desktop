use crate::api_client::{discover, fetch_daemon_version, installed, installed_daemon_version, probe_online};
use crate::SharedState;
use planet_port_api::endpoints;
use serde_json::Value;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// 探活周期：在线状态切换 ≤3s 可感知(spec §5.1)
const TICK: Duration = Duration::from_secs(3);

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || loop {
        let online = probe_online();
        let transitioned = {
            let state = app.state::<SharedState>();
            let mut st = state.lock().unwrap();
            st.installed = installed();
            // 版本面:在线每 tick 从 /version 刷新(升级后即换新);离线仅首轮
            // 跑一次 daemon bin --version 缓存,避免离线期每 tick spawn 进程
            st.daemon_version = if online {
                fetch_daemon_version()
            } else if st.installed {
                st.daemon_version.clone().or_else(installed_daemon_version)
            } else {
                None
            };
            let t = st.online != online;
            st.online = online;
            t
        };
        if online {
            refresh_snapshot(&app);
            crate::tray::refresh(&app);
        }
        if transitioned {
            crate::tray::refresh(&app);
        }
        // 每 tick 常态广播在线状态:前端启动时的一次性读取可能与状态翻转 emit 互相错过
        // (webview 监听器注册晚于翻转),错过后会永久停留离线引导页,常态 emit 保证 ≤3s 收敛
        let _ = app.emit(if online { "daemon-online" } else { "daemon-offline" }, ());
        std::thread::sleep(TICK);
    });
}

/// 在线时每 tick 刷新托盘菜单数据快照(planets + networks 全量，量级小)
fn refresh_snapshot(app: &AppHandle) {
    let Ok(c) = discover() else { return };
    let planets = c.get_json(&endpoints::planets()).ok();
    let networks = c.get_json(&endpoints::networks(None)).ok();
    let state = app.state::<SharedState>();
    let mut st = state.lock().unwrap();
    if let Some(v) = planets {
        st.snapshot_planets = v.get("planets").cloned().unwrap_or_else(|| Value::Array(Vec::new()));
    }
    if let Some(v) = networks {
        st.snapshot_networks = v.get("networks").cloned().unwrap_or_else(|| Value::Array(Vec::new()));
    }
}
