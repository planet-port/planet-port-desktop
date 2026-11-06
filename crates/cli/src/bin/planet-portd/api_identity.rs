//! 身份端点:GET /identity(模式与身份列表)、POST /identity/import
//! (官方身份导入,含停禁官方服务)、PUT /identity(切换全局身份模式)。
//!
//! # 锁与线程纪律
//!
//! - registry 锁短临界区取克隆/改写后即放,不跨 await;
//! - 官方服务停用是子进程阻塞操作,一律 spawn_blocking(红线);
//! - 模式切换要求全部 planet stopped:先取 runtime_gate 再判运行态。
//!   仅查运行时表不够——start_planet 全程持 gate 且「先插 map 再置
//!   running=true」,启动窗口内 map 已有记录而 running 仍为 false,若此时
//!   放行改写 registry,实例会继续用旧身份而登记却是新身份;持 gate 后
//!   与并发 start 串行化,该半途态不再出现。
//! - 身份导入同样受启动窗口影响(身份恰在 NodeHandle::start 内被读/生成),
//!   但停禁官方服务最长 90s,全程持 gate 会把并发 start/stop 一并堵死;
//!   时序为先做覆盖保护与停服务,再持 gate 在门闩内完成 busy 检查与双文件
//!   原子落盘(全程无 await),把 90s 窗口从竞态暴露面里摘掉。

use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};

use crate::registry::IdentityMode;
use crate::service_control::{self, OfficialServiceAction};
use crate::state::{err, runtime_of, AppState};

type ApiErr = (StatusCode, Json<planet_port_api::types::ApiError>);

fn internal(msg: String) -> ApiErr {
    err(StatusCode::INTERNAL_SERVER_ERROR, "internalError", &msg, None)
}

fn bad_request(code: &str, msg: &str) -> ApiErr {
    err(StatusCode::BAD_REQUEST, code, msg, None)
}

/// identity.secret/public 首字段(冒号前)即十位 hex 节点地址
fn identity_addr(bytes: &[u8]) -> Option<String> {
    let s = std::str::from_utf8(bytes).ok()?;
    s.split(':')
        .next()
        .map(|a| a.trim().to_lowercase())
        .filter(|a| a.len() == 10 && a.chars().all(|c| c.is_ascii_hexdigit()))
}

/// 官方客户端数据目录(以 identity.secret 存在为准);未安装返回 None。
/// macOS 官方客户端(pkg/Homebrew cask)的工作目录在 .../ZeroTier/One,
/// 兼容历史/变体布局按序探测候选目录。
fn official_dir() -> Option<PathBuf> {
    let candidates: Vec<PathBuf> = if cfg!(target_os = "macos") {
        vec![
            PathBuf::from("/Library/Application Support/ZeroTier/One"),
            PathBuf::from("/Library/Application Support/ZeroTier"),
        ]
    } else if cfg!(target_os = "linux") {
        vec![PathBuf::from("/var/lib/zerotier-one")]
    } else if cfg!(target_os = "windows") {
        vec![PathBuf::from(r"C:\ProgramData\ZeroTier\One")]
    } else {
        return None;
    };
    candidates
        .into_iter()
        .find(|d| d.join("identity.secret").exists())
}

/// 全局身份节点地址:优先 identity.public,回落 identity.secret
fn global_node_id(config_dir: &std::path::Path) -> Option<String> {
    fs::read(config_dir.join("identity.public"))
        .ok()
        .as_deref()
        .and_then(identity_addr)
        .or_else(|| {
            fs::read(config_dir.join("identity.secret"))
                .ok()
                .as_deref()
                .and_then(identity_addr)
        })
}

fn mode_str(m: IdentityMode) -> &'static str {
    match m {
        IdentityMode::Shared => "shared",
        IdentityMode::Independent => "independent",
    }
}

// ---- GET /identity ----

pub async fn identity_list(State(st): State<AppState>) -> Result<Json<Value>, ApiErr> {
    let (entries, mode) = {
        let reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        (reg.planets.clone(), reg.identity_mode)
    };
    let exists = st.config_dir.join("identity.secret").exists();
    let g_node = global_node_id(&st.config_dir);
    let planets: Vec<Value> = entries
        .iter()
        .map(|e| {
            // independent 读 dataDir 内 identity.public(显式 stop 后
            // last_node_id 会被置 None,身份本体留盘可解析,列表不丢地址);
            // 取不到回落 last_node_id,再无则 null。shared 即全局身份地址。
            let node_id = match mode {
                IdentityMode::Independent => fs::read(e.data_dir.join("identity.public"))
                    .ok()
                    .as_deref()
                    .and_then(identity_addr)
                    .or_else(|| e.last_node_id.clone()),
                IdentityMode::Shared => g_node.clone(),
            };
            json!({
                "id": e.id,
                "name": crate::api_planet::name_of(e),
                "nodeId": node_id,
                "running": e.running,
            })
        })
        .collect();
    Ok(Json(json!({
        "mode": mode_str(mode),
        "global": { "exists": exists, "nodeId": g_node },
        "planets": planets,
    })))
}

/// 解析官方客户端当前使用的 planet 文件(身份同目录下的 `planet`),供
/// 导入响应提示"原网络挂在哪个 planet、是否已导入"。缺失/非 planet
/// (如 moon)/解析失败一律 None,不影响导入主流程。
fn official_planet_info(secret_path: &std::path::Path) -> Option<Value> {
    let path = secret_path.with_file_name("planet");
    let bytes = fs::read(&path).ok()?;
    let info = crate::world::parse_world(&bytes).ok()?;
    if info.kind != crate::world::WorldKind::Planet {
        return None;
    }
    Some(json!({
        "worldId": info.world_id,
        "official": info.world_id == planet_port_api::types::OFFICIAL_WORLD_ID,
        "path": path,
    }))
}

// ---- POST /identity/import ----

pub async fn identity_import(
    State(st): State<AppState>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let force = body.get("force").and_then(|v| v.as_bool()).unwrap_or(false);
    // 1. 定位身份文件:显式 path(identity.secret 路径或其目录)或自动探测
    let secret_path = match body.get("path").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() => {
            let pb = PathBuf::from(p);
            if pb.is_dir() { pb.join("identity.secret") } else { pb }
        }
        _ => official_dir()
            .ok_or_else(|| {
                bad_request(
                    "officialNotFound",
                    "cannot detect official ZeroTier data dir; pass \"path\"",
                )
            })?
            .join("identity.secret"),
    };
    let public_path = secret_path.with_file_name("identity.public");
    // 2. 校验:两文件都在、地址可解析且一致
    let secret = fs::read(&secret_path).map_err(|e| {
        bad_request(
            "sourceUnreadable",
            &format!("cannot read {}: {e}", secret_path.display()),
        )
    })?;
    let public = fs::read(&public_path).map_err(|e| {
        bad_request(
            "sourceUnreadable",
            &format!("cannot read {}: {e}", public_path.display()),
        )
    })?;
    let addr_s = identity_addr(&secret)
        .ok_or_else(|| bad_request("invalidIdentity", "cannot parse identity.secret address"))?;
    // ZeroTier 身份串格式(vendored Core/node/Identity.cpp toString):
    // identity.public = 「addr:0:<pubhex>」3 段;identity.secret =
    // 「addr:0:<pubhex>:<privhex>」4 段。第 4 段(下标 3)是私钥段:
    // 误传 identity.public 时双文件同名同内容、地址校验全过,公钥一旦落盘会
    // 污染 config 根的 identity.secret,故此处校验确实含私钥段。
    if std::str::from_utf8(&secret)
        .ok()
        .and_then(|s| s.split(':').nth(3))
        .is_none_or(|priv_part| priv_part.trim().is_empty())
    {
        return Err(bad_request(
            "invalidIdentity",
            "file looks like identity.public, not identity.secret",
        ));
    }
    let addr_p = identity_addr(&public)
        .ok_or_else(|| bad_request("invalidIdentity", "cannot parse identity.public address"))?;
    if addr_s != addr_p {
        return Err(bad_request(
            "invalidIdentity",
            "identity.secret and identity.public addresses do not match",
        ));
    }
    // 3. 覆盖保护:已有全局身份且未 force → 拒绝
    let dest_secret = st.config_dir.join("identity.secret");
    let dest_public = st.config_dir.join("identity.public");
    if dest_secret.exists() && !force {
        return Err(err(
            StatusCode::CONFLICT,
            "identityExists",
            "global identity already exists; pass force to overwrite",
            None,
        ));
    }
    // 4. 停+禁官方服务(子进程,放阻塞线程池);失败即中止导入。
    //    放在持 gate 之前:该操作最长 90s,门闩内做会把并发 start/stop
    //    一并堵死;停完再进门闩,运行态判定与落盘之间不再有可插入窗口。
    let action = tokio::task::spawn_blocking(service_control::stop_and_disable)
        .await
        .map_err(|e| internal(format!("service stop 任务失败: {e}")))?;
    if let OfficialServiceAction::Failed(e) = &action {
        return Err(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "officialServiceStopFailed",
            &format!(
                "cannot stop official ZeroTier service ({e}); import aborted, \
                 stop it manually and retry"
            ),
            None,
        ));
    }
    // 3.5 运行态守卫:任何 planet 运行中都不允许导入(尤其 --force 覆盖
    // 共享身份)——否则出现「实例用旧地址、列表显新地址」的分裂,先拒绝
    // 并指引逐个 stop。判定 = runtime_of(实际运行)+ registry.running
    // (期望状态,启动窗口内 runtime 表可能有空档)。此时正持 runtime_gate:
    // start_planet 全程持 gate 且身份在 NodeHandle::start 内被读/生成,仅查
    // 运行态会在整个启动窗口双盲,持 gate 后与并发 start 串行化,从根上
    // 消除「实例用旧身份、config 根已是新身份」的分裂。锁纪律:短临界区——
    // 锁内只取 id/name/布尔即放,runtime_of 另锁 planets 表,不嵌套持锁;
    // 门闩内(含后续落盘)全程无 await。
    let _gate = st.runtime_gate.lock().await;
    let all_planets: Vec<(String, bool, String)> = {
        let reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        reg.planets
            .iter()
            .map(|e| (e.id.clone(), e.running, crate::api_planet::name_of(e)))
            .collect()
    };
    let busy: Vec<String> = all_planets
        .into_iter()
        .filter(|(id, running, _)| *running || runtime_of(&st, id).is_some())
        .map(|(id, _, name)| format!("{name}(id {id})"))
        .collect();
    if !busy.is_empty() {
        return Err(err(
            StatusCode::CONFLICT,
            "globalIdentityInUse",
            &format!(
                "global identity is in use by running planet(s): {}; \
                 disable them first (`planet-port planet disable <planet>`) and retry",
                busy.join(", ")
            ),
            None,
        ));
    }
    // 4.5 gate 内重判覆盖保护:步骤 3 的判定发生在停服务(最长 90s)之前,
    // 并发的两个非 force 导入都可能先过守卫再先后进门闩,后者会静默覆盖
    // 前者。此处正持门闩、紧邻落盘,重判一次把窗口封死;前置判定保留作
    // fail-fast,避免无谓停官方服务。
    if dest_secret.exists() && !force {
        return Err(err(
            StatusCode::CONFLICT,
            "identityExists",
            "global identity already exists; pass force to overwrite",
            None,
        ));
    }
    // 5. 写盘:secret/public 两文件必须成对生效。先各自写好不同名的 tmp
    // (不能用 with_extension("tmp"),它会把 identity.secret/public 都变成
    // identity.tmp 而互相覆盖),每个 tmp 写完立即收紧到 0600(rename 保留
    // 权限),消除「先 0644 落盘、后补 chmod」的明文窗口;chmod 失败不留
    // 半态:清理 tmp、events 留痕并以 internal 错误中止。rename 前先把旧
    // dest 各自挪成 .bak(secret 成功而 public 失败时旧 public 仍可整对
    // 复原),两个 tmp 都 rename 成功才删 .bak;任一 rename 失败则把已
    // rename 的 dest 与 .bak 复原回原名、清理 tmp 并以 internal 错误中止,
    // 磁盘不留「新 secret 配旧 public」的半态。
    let tmp_for = |dest: &std::path::Path| -> PathBuf {
        let name = dest.file_name().and_then(|n| n.to_str()).unwrap_or("identity");
        dest.with_file_name(format!("{name}.tmp"))
    };
    let tmp_secret = tmp_for(&dest_secret);
    let tmp_public = tmp_for(&dest_public);
    let cleanup_tmps = || {
        let _ = fs::remove_file(&tmp_secret);
        let _ = fs::remove_file(&tmp_public);
    };
    let tighten = |tmp: &std::path::Path| -> Result<(), std::io::Error> {
        #[cfg(unix)]
        {
            fs::set_permissions(tmp, fs::Permissions::from_mode(0o600))
        }
        #[cfg(not(unix))]
        {
            let _ = tmp;
            Ok(())
        }
    };
    for (tmp, payload) in [(&tmp_secret, &secret), (&tmp_public, &public)] {
        if let Err(e) = fs::write(tmp, payload) {
            cleanup_tmps();
            return Err(internal(format!("cannot write {}: {e}", tmp.display())));
        }
        if let Err(e) = tighten(tmp) {
            cleanup_tmps();
            st.events.push(
                "global",
                "identitySecretPermFailed",
                &format!("cannot tighten {} to 0600: {e}", tmp.display()),
            );
            return Err(internal(format!(
                "cannot tighten {} to 0600: {e}",
                tmp.display()
            )));
        }
    }
    // rename 前备份旧 dest:rename 是覆盖式语义,旧 identity.secret/public
    // 一旦被第一个 rename 吃掉就无法凭自身复原,只能靠 .bak
    let bak_for = |dest: &std::path::Path| -> PathBuf {
        let name = dest.file_name().and_then(|n| n.to_str()).unwrap_or("identity");
        dest.with_file_name(format!("{name}.bak"))
    };
    let bak_secret = bak_for(&dest_secret);
    let bak_public = bak_for(&dest_public);
    let stage_bak =
        |dest: &std::path::Path, bak: &std::path::Path| -> Result<(), std::io::Error> {
            if dest.exists() {
                fs::rename(dest, bak)
            } else {
                Ok(())
            }
        };
    // 归位单个 dest:.bak 存在才 rename 回原名;归位失败不吞错,留痕
    // identityRestoreFailed 并返回 false,让调用方如实上报回滚结果
    let restore = |bak: &std::path::Path, dest: &std::path::Path| -> bool {
        if !bak.exists() {
            return true;
        }
        match fs::rename(bak, dest) {
            Ok(()) => true,
            Err(e) => {
                st.events.push(
                    "global",
                    "identityRestoreFailed",
                    &format!("cannot restore {} from {}: {e}", dest.display(), bak.display()),
                );
                false
            }
        }
    };
    let restore_all = || {
        let secret_ok = restore(&bak_secret, &dest_secret);
        let public_ok = restore(&bak_public, &dest_public);
        secret_ok && public_ok
    };
    if let Err(e) = stage_bak(&dest_secret, &bak_secret).and_then(|()| stage_bak(&dest_public, &bak_public)) {
        // secret 已挪成 .bak 而原名缺位,不归位会让下次 start 读不到
        // secret 而生成全新身份、静默换地址,必须先复原再中止
        restore_all();
        cleanup_tmps();
        return Err(internal(format!("cannot back up existing identity: {e}")));
    }
    // 各 dest 是否有旧身份(即有 .bak):回滚时 .bak 归位会覆盖掉已写入
    // 的新文件;首次导入无 .bak,已写入的 dest 是孤儿新文件,须删除
    let had_bak = [bak_secret.exists(), bak_public.exists()];
    let mut written: Vec<(&std::path::Path, bool)> = Vec::new();
    for (i, (tmp, dest)) in [(&tmp_secret, &dest_secret), (&tmp_public, &dest_public)]
        .into_iter()
        .enumerate()
    {
        if let Err(e) = fs::rename(tmp, dest) {
            // 失败复原:有 .bak 的 dest 用 .bak 归位覆盖新文件,无 .bak
            // 的已写入 dest 删除孤儿,tmp 一并清理不留垃圾
            let rollback_ok = restore_all();
            for (d, bak) in &written {
                if !*bak {
                    if let Err(del_err) = fs::remove_file(d) {
                        st.events.push(
                            "global",
                            "identityRestoreFailed",
                            &format!("cannot remove orphaned {}: {del_err}", d.display()),
                        );
                    }
                }
            }
            cleanup_tmps();
            return Err(internal(format!(
                "cannot rename to {} ({e}); {}",
                dest.display(),
                if rollback_ok {
                    "previous identity restored"
                } else {
                    "rollback attempted; check events / identityRestoreFailed"
                }
            )));
        }
        written.push((dest, had_bak[i]));
    }
    let _ = fs::remove_file(&bak_secret);
    let _ = fs::remove_file(&bak_public);
    st.events
        .push("global", "identityImported", &format!("global identity imported: {addr_s}"));
    let action_str = match action {
        OfficialServiceAction::StoppedAndDisabled => "stoppedAndDisabled",
        OfficialServiceAction::NotInstalled => "notInstalled",
        OfficialServiceAction::Failed(_) => unreachable!("failed 已在上方拦截"),
    };
    // 官方客户端当前 planet 的识别与已注册比对:身份迁移保留了原节点
    // 地址,成员资格随之保留,但"原网络挂在哪个 planet"只有官方目录里
    // 的 `planet` 文件知道。报给 CLI 才能提示"直接 join 恢复"还是
    // "先 add 该 planet"。锁纪律:短临界区取名字即放,不嵌套锁。
    let official_planet = official_planet_info(&secret_path).map(|mut v| {
        let reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(w) = v["worldId"].as_u64() {
            if let Some(e) = reg.planets.iter().find(|e| e.world_id == w) {
                v["registeredName"] = json!(crate::api_planet::name_of(e));
            }
        }
        v
    });
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "nodeId": addr_s,
            "serviceAction": action_str,
            "officialPlanet": official_planet,
        })),
    ))
}

// ---- PUT /identity ----

pub async fn identity_mode_set(
    State(st): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiErr> {
    let mode = match body.get("mode").and_then(|v| v.as_str()) {
        Some("shared") => IdentityMode::Shared,
        Some("independent") => IdentityMode::Independent,
        _ => {
            return Err(bad_request(
                "badRequest",
                "body must contain \"mode\": \"shared\" | \"independent\"",
            ))
        }
    };
    // 先取启停门闩(与原行星级切换同理):start_planet 先插运行时表再置
    // running=true,启动窗口内只查运行时表会漏判运行中,持 gate 后判定
    // 与 registry 改写都被串行化,不会与并发 start 交错。门闩内不做 await。
    let _gate = st.runtime_gate.lock().await;
    // 模式影响全部 planet 的身份来源:任何 planet 运行中即拒绝
    let all_planets: Vec<(String, bool, String)> = {
        let reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        reg.planets
            .iter()
            .map(|e| (e.id.clone(), e.running, crate::api_planet::name_of(e)))
            .collect()
    };
    let busy: Vec<String> = all_planets
        .into_iter()
        .filter(|(id, running, _)| *running || runtime_of(&st, id).is_some())
        .map(|(id, _, name)| format!("{name}(id {id})"))
        .collect();
    if !busy.is_empty() {
        return Err(err(
            StatusCode::CONFLICT,
            "planetRunning",
            &format!(
                "planet(s) running: {}; stop them before switching identity mode",
                busy.join(", ")
            ),
            None,
        ));
    }
    {
        let mut reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        reg.identity_mode = mode;
        if let Err(e) = reg.save(&st.config_dir) {
            // 半态可见:模式已改在内存生效但未落盘,事件留痕供 events 排查
            eprintln!("[identity] registry 写盘失败: {e}");
            st.events
                .push("global", "identityModePersistFailed", &e.to_string());
        }
    }
    st.events.push(
        "global",
        "identityModeSwitched",
        &format!("identity mode switched to {}", mode_str(mode)),
    );
    Ok(Json(json!({ "result": "ok", "mode": mode_str(mode) })))
}

/// 身份路由:/identity(GET 列表 + PUT 模式切换)、/identity/import
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/identity", get(identity_list).put(identity_mode_set))
        .route("/identity/import", post(identity_import))
}
