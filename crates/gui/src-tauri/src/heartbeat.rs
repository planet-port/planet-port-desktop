use crate::api_client::{discover, installed, probe_online};
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
            let t = st.online != online;
            st.online = online;
            t
        };
        if online {
            refresh_snapshot(&app);
            // Task 9 接入托盘后此处追加: crate::tray::refresh(&app);
        }
        if transitioned {
            let _ = app.emit(if online { "daemon-online" } else { "daemon-offline" }, ());
            // Task 9 接入托盘后此处追加: crate::tray::refresh(&app);
        }
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
