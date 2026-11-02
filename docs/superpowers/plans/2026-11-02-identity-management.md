# 身份管理层实施计划(全局身份 / 每 planet 绑定 / 官方身份迁移)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 落地 docs/superpowers/specs/2026-11-02-identity-management-design.md ——zt-core 身份路径映射注入、registry 每 planet 身份绑定、官方身份导入(自动停禁官方服务)、identity REST 端点与 CLI。

**Architecture:** zt-core 的 `NodeParams` 新增 `identity_dir: Option<PathBuf>`,StateStore 把 `IdentityPublic/Secret` 映射到该目录(None = 现状 dataDir 根,即 own 绑定);daemon 依据 registry 每 planet 的 `identity` 绑定决定注入。官方身份导入 = 校验 → 停+禁官方服务 → 写 `<config>/identity.*`。CLI 新增 `identity` 命令(list/import/use)。

**Tech Stack:** Rust workspace(axum daemon + clap CLI + zt-core FFI 封装),无新依赖。

## Global Constraints

- 分支:worktree 内 `feat/identity-management`(基于 dev 02d2ba2),全程不 push。
- **不写测试用例**(用户规矩):每个任务的验证 = `cargo build --workspace` 通过 + `cargo clippy --workspace --all-targets -- -D warnings` 无告警。
- 提交信息遵循 Conventional Commits:type/scope 英文、description 与 Body 中文、必须 Body、禁止 Co-authored-by。
- daemon 锁纪律(红线):registry/planets 锁只短临界区取克隆,不跨 `await`、不嵌套;阻塞命令(子进程、orbit 类)一律 `spawn_blocking`。
- zt-core StateStore 红线:只在核心回调内被调用,禁止 panic/unwrap;identity.secret 落盘 0600(Windows 无 POSIX 位,跳过,由目录 ACL 承担)。
- 独立身份与全局身份互不继承:切 own 不拷贝全局身份;own 身份只存在于 planet dataDir,切 global 后原文件留盘不删。
- 注释风格对齐周边中文注释;文件路径引用均相对仓库根。

---

### Task 1: zt-core 身份路径映射注入

**Files:**
- Modify: `crates/zt-core/src/config.rs:16-21`(NodeParams)
- Modify: `crates/zt-core/src/state.rs:18-44`(StateStore 构造与 file_for)
- Modify: `crates/zt-core/src/node.rs:339`(StateStore::new 调用)
- Modify: `crates/daemon/src/runtime.rs:449`(NodeParams 构造,本任务先补 None)

**Interfaces:**
- Produces: `NodeParams { data_dir: PathBuf, planet_file: PathBuf, bind_port: u16, identity_dir: Option<PathBuf> }`(后续任务依赖此签名);`StateStore::new(root: PathBuf, world_id: u64, planet_in: PathBuf, identity_dir: Option<PathBuf>)`。

- [ ] **Step 1: config.rs 给 NodeParams 加字段**

在 `bind_port: u16,` 之后加:

```rust
    /// 身份文件目录(None = data_dir 根,每 planet 独立身份;
    /// Some = 全局身份目录,daemon 侧按 registry 绑定注入)
    pub identity_dir: Option<PathBuf>,
```

- [ ] **Step 2: state.rs 构造与映射**

`StateStore` 结构体加字段 `identity_dir: Option<PathBuf>`(放在 `root` 之后);`new` 签名与构造相应加参:

```rust
    pub fn new(
        root: PathBuf,
        world_id: u64,
        planet_in: PathBuf,
        identity_dir: Option<PathBuf>,
    ) -> Self {
        let world_dir = root.join(world_id.to_string());
        Self { root, identity_dir, world_dir, planet_in }
    }
```

`file_for` 的 identity 两分支改为(identity_dir 优先,回落 dataDir 根 = 现状语义):

```rust
            IdentityPublic => Some(
                self.identity_dir
                    .clone()
                    .unwrap_or_else(|| self.root.clone())
                    .join("identity.public"),
            ),
            IdentitySecret => Some(
                self.identity_dir
                    .clone()
                    .unwrap_or_else(|| self.root.clone())
                    .join("identity.secret"),
            ),
```

同时把 `state.rs` 顶部模块注释第 2 行「identity 跨 planet 共享于根」修正为「identity 落 data_dir 根(独立)或注入的全局身份目录(identity_dir)」。

- [ ] **Step 3: node.rs 传参**

`crates/zt-core/src/node.rs:339` 的调用改为:

```rust
            store: StateStore::new(
                params.data_dir.clone(),
                world_id,
                params.planet_file.clone(),
                params.identity_dir.clone(),
            ),
```

- [ ] **Step 4: daemon 侧补编译**

`crates/daemon/src/runtime.rs:449` 的 `NodeParams { ... }` 补一行(Task 2 会替换为真值):

```rust
        identity_dir: None,
```

- [ ] **Step 5: 构建验证**

Run: `cargo build --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 全部通过(行为零变化:所有调用方 identity_dir 均为 None)。

- [ ] **Step 6: Commit**

```bash
git add crates/zt-core/src/config.rs crates/zt-core/src/state.rs crates/zt-core/src/node.rs crates/daemon/src/runtime.rs
git commit -m "feat(zt-core): NodeParams 支持注入全局身份目录

StateStore 的 identity 落盘路径改为 identity_dir 优先、data_dir 根回落,
为 daemon 按 registry 绑定切换全局/独立身份提供注入点;缺省 None 时
行为与现状完全一致。"
```

---

### Task 2: registry 身份绑定字段与启动注入

**Files:**
- Modify: `crates/daemon/src/registry.rs:12-49`
- Modify: `crates/daemon/src/api_planet.rs:374-395`(import 时 RegistryEntry 构造)
- Modify: `crates/daemon/src/runtime.rs:449-454`(NodeParams 注入)

**Interfaces:**
- Produces: `crate::registry::IdentityBinding`(enum Own/Global,serde 小写,Default=Own);`RegistryEntry.identity: IdentityBinding`。Task 4 的 API 与 Task 5 的 CLI 依赖此字段语义("own"/"global")。

- [ ] **Step 1: registry.rs 加绑定枚举与字段**

在 `RegistryEntry` 定义之前加:

```rust
/// planet 的节点身份绑定:Own = dataDir 内独立身份(存量/缺省语义);
/// Global = 共享 config 根的全局身份(identity.public/secret)。
/// 存量 registry.json 无此字段,反序列化回落 Own。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IdentityBinding {
    Own,
    Global,
}

impl Default for IdentityBinding {
    fn default() -> Self {
        Self::Own
    }
}
```

`RegistryEntry` 的 `pub ifaces: Vec<String>,` 之后加:

```rust
    /// 节点身份绑定(own/global);缺省 Own 兼容存量注册表
    #[serde(default)]
    pub identity: IdentityBinding,
```

- [ ] **Step 2: import 默认绑定**

`crates/daemon/src/api_planet.rs` 的 `RegistryEntry { ... }` 构造(`:374`)里 `ifaces: Vec::new(),` 之后加(存在全局身份则新 planet 默认绑定它):

```rust
        identity: if st.config_dir.join("identity.secret").exists() {
            IdentityBinding::Global
        } else {
            IdentityBinding::Own
        },
```

同文件 `use crate::registry::...` 处补 `IdentityBinding`。

- [ ] **Step 3: start 注入 identity_dir**

`crates/daemon/src/runtime.rs:449` 的 `NodeParams` 构造把 Task 1 的 `identity_dir: None` 替换为:

```rust
        identity_dir: match entry.identity {
            IdentityBinding::Global => Some(state.config_dir.clone()),
            IdentityBinding::Own => None,
        },
```

文件顶部 `use crate::registry::{...}` 补 `IdentityBinding`(若 runtime.rs 经其他路径引用 registry 类型,按现有风格对齐)。

- [ ] **Step 4: 构建验证**

Run: `cargo build --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 通过。存量 registry.json 反序列化不报错(serde default)。

- [ ] **Step 5: Commit**

```bash
git add crates/daemon/src/registry.rs crates/daemon/src/api_planet.rs crates/daemon/src/runtime.rs
git commit -m "feat(daemon): planet 注册表新增身份绑定字段

registry 每 planet 记录 identity(own/global),存量无字段回落 own;
导入 planet 时存在全局身份则默认绑定 global;start 据此向 zt-core
注入 identity_dir,global 绑定即读 config 根全局身份。"
```

---

### Task 3: 官方 ZeroTier 服务停用模块

**Files:**
- Create: `crates/daemon/src/service_control.rs`
- Modify: `crates/daemon/src/main.rs:6-17`(mod 声明)

**Interfaces:**
- Produces: `crate::service_control::{stop_and_disable() -> OfficialServiceAction, OfficialServiceAction::{StoppedAndDisabled, NotInstalled, Failed(String)}}`。Task 4 的 import 端点消费。阻塞实现(子进程),调用方必须放 `spawn_blocking`。

- [ ] **Step 1: 新建 service_control.rs**

```rust
//! 官方 ZeroTier 服务的探测与停用(stop + disable 自启,不卸载)。
//! 供身份导入在写入全局身份前调用,消除同身份双开互踢(官方 issue #2433)。
//! 全部为子进程阻塞实现,调用方须放 spawn_blocking(锁与线程纪律红线)。

use std::path::Path;
use std::process::Command;

/// 官方服务处置结果(not_installed = 本机未装官方客户端,导入照常)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OfficialServiceAction {
    StoppedAndDisabled,
    NotInstalled,
    Failed(String),
}

/// 跑子进程:成功返回 stdout,失败返回带程序名的错误串
fn run(cmd: &mut Command) -> Result<String, String> {
    let out = cmd
        .output()
        .map_err(|e| format!("{:?}: {e}", cmd.get_program()))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!(
            "{:?}: {}{}",
            cmd.get_program(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        ))
    }
}

#[cfg(target_os = "macos")]
pub fn stop_and_disable() -> OfficialServiceAction {
    // 探测:launchd 已加载或 plist 在盘即视为安装
    let installed = Command::new("launchctl")
        .args(["print", "system/com.zerotier.one"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
        || Path::new("/Library/LaunchDaemons/com.zerotier.one.plist").exists();
    if !installed {
        return OfficialServiceAction::NotInstalled;
    }
    // 未加载时 bootout 报错属预期,忽略其错误继续 disable
    let _ = run(Command::new("launchctl").args(["bootout", "system/com.zerotier.one"]));
    match run(Command::new("launchctl").args(["disable", "system/com.zerotier.one"])) {
        Ok(_) => OfficialServiceAction::StoppedAndDisabled,
        Err(e) => OfficialServiceAction::Failed(e),
    }
}

#[cfg(target_os = "linux")]
pub fn stop_and_disable() -> OfficialServiceAction {
    let installed = match run(Command::new("systemctl").args(["list-unit-files", "zerotier-one.service"])) {
        Ok(out) => out.contains("zerotier-one"),
        Err(_) => false,
    };
    if !installed {
        return OfficialServiceAction::NotInstalled;
    }
    // 已停止时 stop 为幂等成功
    if let Err(e) = run(Command::new("systemctl").args(["stop", "zerotier-one"])) {
        return OfficialServiceAction::Failed(e);
    }
    match run(Command::new("systemctl").args(["disable", "zerotier-one"])) {
        Ok(_) => OfficialServiceAction::StoppedAndDisabled,
        Err(e) => OfficialServiceAction::Failed(e),
    }
}

#[cfg(target_os = "windows")]
pub fn stop_and_disable() -> OfficialServiceAction {
    let installed = Command::new("sc.exe")
        .args(["query", "ZeroTierOneService"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !installed {
        return OfficialServiceAction::NotInstalled;
    }
    // 已停止时 stop 报非运行错误,忽略继续 disable
    let _ = run(Command::new("sc.exe").args(["stop", "ZeroTierOneService"]));
    match run(Command::new("sc.exe").args(["config", "ZeroTierOneService", "start=", "disabled"])) {
        Ok(_) => OfficialServiceAction::StoppedAndDisabled,
        Err(e) => OfficialServiceAction::Failed(e),
    }
}
```

- [ ] **Step 2: main.rs 挂模块**

`crates/daemon/src/main.rs` 的 `mod runtime;` 之后加一行 `mod service_control;`。

- [ ] **Step 3: 构建验证**

Run: `cargo build --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 通过(macOS 上编译当前平台分支;`Failed` 变体等在其他平台分支使用,本平台下如有 dead_code 告警,给 `stop_and_disable` 加 `#[allow(dead_code)]` 会被 Task 4 消费——若 clippy 报 unused,临时允许并在 Task 4 移除)。

- [ ] **Step 4: Commit**

```bash
git add crates/daemon/src/service_control.rs crates/daemon/src/main.rs
git commit -m "feat(daemon): 新增官方 ZeroTier 服务停用模块

跨平台(macOS launchctl / Linux systemctl / Windows sc)探测并
stop+disable 官方服务但不卸载,返回三态结果;供身份导入在写盘前
调用,防止同身份双开互踢。"
```

---

### Task 4: 身份 REST 端点

**Files:**
- Create: `crates/daemon/src/api_identity.rs`
- Modify: `crates/daemon/src/main.rs`(mod 声明 + router merge)
- Modify: `crates/api/src/endpoints.rs`(3 个路径构造)
- Modify: `crates/api/src/types.rs`(响应类型,供 CLI 反序列化)

**Interfaces:**
- Consumes: Task 2 的 `IdentityBinding`/`RegistryEntry.identity`;Task 3 的 `service_control::stop_and_disable`。
- Produces: REST 契约——
  - `GET /identity` → `{"global":{"exists":bool,"nodeId":"xxxxxxxxxx"|null},"planets":[{"id","name","identity":"own"|"global","nodeId",...running}]}`(类型 `planet_port_api::types::{GlobalIdentity, PlanetIdentity}`)
  - `POST /identity/import` body `{"path"?:string,"force"?:bool}` → 201 `{"nodeId","serviceAction":"stoppedAndDisabled"|"notInstalled"}`;错误码:`officialNotFound`(400,探测不到且未给 path)、`sourceUnreadable`(400)、`invalidIdentity`(400,解析失败/地址不符)、`identityExists`(409,已有全局身份且未 force)、`officialServiceStopFailed`(500,停服务失败已中止)
  - `PUT /planets/{id}/identity` body `{"identity":"own"|"global"}` → `{"result":"ok"}`;运行中 409 `planetRunning`;未知 planet 404 `planetNotFound`

- [ ] **Step 1: types.rs 加响应类型**

`crates/api/src/types.rs` 末尾(derive 与 serde 属性对齐邻近视 MoonEntry/PlanetStatus 风格):

```rust
/// GET /identity 的全局身份段
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalIdentity {
    pub exists: bool,
    /// 十位 hex 节点地址;全局身份缺失或不可解析时 None
    pub node_id: Option<String>,
}

/// GET /identity 的单 planet 段
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanetIdentity {
    pub id: String,
    pub name: String,
    /// "own" | "global"
    pub identity: String,
    pub node_id: Option<String>,
    pub running: bool,
}
```

- [ ] **Step 2: endpoints.rs 加路径构造**

`crates/api/src/endpoints.rs` 的 `dump()` 之前加:

```rust
pub fn identity() -> &'static str {
    "/identity"
}
pub fn identity_import() -> &'static str {
    "/identity/import"
}
pub fn planet_identity(id: &str) -> String {
    format!("/planets/{}/identity", enc(id))
}
```

- [ ] **Step 3: 新建 api_identity.rs**

```rust
//! 身份端点:GET /identity(模式与身份列表)、POST /identity/import
//! (官方身份导入,含停禁官方服务)、PUT /planets/{id}/identity(切换绑定)。
//! 语义见 docs/superpowers/specs/2026-11-02-identity-management-design.md。
//!
//! # 锁与线程纪律
//!
//! - registry 锁短临界区取克隆/改写后即放,不跨 await;
//! - 官方服务停用是子进程阻塞操作,一律 spawn_blocking(红线);
//! - 切换绑定要求 planet stopped:registry.running 与 planets 运行时表都查。

use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post, put},
    Json, Router,
};
use serde_json::{json, Value};

use crate::registry::IdentityBinding;
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

/// 官方客户端数据目录(以 identity.secret 存在为准);未安装返回 None
fn official_dir() -> Option<PathBuf> {
    let dir = if cfg!(target_os = "macos") {
        PathBuf::from("/Library/Application Support/ZeroTier")
    } else if cfg!(target_os = "linux") {
        PathBuf::from("/var/lib/zerotier-one")
    } else if cfg!(target_os = "windows") {
        PathBuf::from(r"C:\ProgramData\ZeroTier\One")
    } else {
        return None;
    };
    dir.join("identity.secret").exists().then_some(dir)
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

fn binding_str(b: IdentityBinding) -> &'static str {
    match b {
        IdentityBinding::Own => "own",
        IdentityBinding::Global => "global",
    }
}

// ---- GET /identity ----

pub async fn identity_list(State(st): State<AppState>) -> Result<Json<Value>, ApiErr> {
    let entries = {
        let reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        reg.planets.clone()
    };
    let exists = st.config_dir.join("identity.secret").exists();
    let g_node = global_node_id(&st.config_dir);
    let planets: Vec<Value> = entries
        .iter()
        .map(|e| {
            // own 绑定看自己的 last_node_id;global 绑定即全局身份地址
            let node_id = match e.identity {
                IdentityBinding::Own => e.last_node_id.clone(),
                IdentityBinding::Global => g_node.clone(),
            };
            json!({
                "id": e.id,
                "name": crate::api_planet::name_of(e),
                "identity": binding_str(e.identity),
                "nodeId": node_id,
                "running": e.running,
            })
        })
        .collect();
    Ok(Json(json!({
        "global": { "exists": exists, "nodeId": g_node },
        "planets": planets,
    })))
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
    // 4. 停+禁官方服务(子进程,放阻塞线程池);失败即中止导入
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
    // 5. 写盘(先 tmp 后 rename,对齐 StateStore/registry 原子写模式)
    for (dest, bytes) in [(&dest_secret, &secret), (&dest_public, &public)] {
        let tmp = dest.with_extension("tmp");
        fs::write(&tmp, bytes)
            .map_err(|e| internal(format!("cannot write {}: {e}", tmp.display())))?;
        if let Err(e) = fs::rename(&tmp, dest) {
            let _ = fs::remove_file(&tmp);
            return Err(internal(format!("cannot rename to {}: {e}", dest.display())));
        }
        #[cfg(unix)]
        let _ = fs::set_permissions(dest, fs::Permissions::from_mode(0o600));
    }
    st.events
        .push("global", "identityImported", &format!("global identity imported: {addr_s}"));
    let action_str = match action {
        OfficialServiceAction::StoppedAndDisabled => "stoppedAndDisabled",
        OfficialServiceAction::NotInstalled => "notInstalled",
        OfficialServiceAction::Failed(_) => unreachable!("failed 已在上方拦截"),
    };
    Ok((
        StatusCode::CREATED,
        Json(json!({ "nodeId": addr_s, "serviceAction": action_str })),
    ))
}

// ---- PUT /planets/{id}/identity ----

pub async fn planet_identity_set(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiErr> {
    let binding = match body.get("identity").and_then(|v| v.as_str()) {
        Some("own") => IdentityBinding::Own,
        Some("global") => IdentityBinding::Global,
        _ => {
            return Err(bad_request(
                "badRequest",
                "body must contain \"identity\": \"own\" | \"global\"",
            ))
        }
    };
    // 运行中拒绝:registry 标志与运行时表双查(与 DELETE 同款防御)
    if runtime_of(&st, &id).is_some() {
        return Err(err(
            StatusCode::CONFLICT,
            "planetRunning",
            &format!("planet \"{id}\" is running; stop it before switching identity"),
            None,
        ));
    }
    {
        let mut reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(e) = reg.planets.iter_mut().find(|e| e.id == id) else {
            return Err(err(
                StatusCode::NOT_FOUND,
                "planetNotFound",
                &format!("planet \"{id}\" not found"),
                None,
            ));
        };
        e.identity = binding;
        if let Err(e) = reg.save(&st.config_dir) {
            eprintln!("[identity] registry 写盘失败: {e}");
        }
    }
    st.events.push(
        &id,
        "identitySwitch",
        &format!("identity binding switched to {}", binding_str(binding)),
    );
    Ok(Json(json!({ "result": "ok", "id": id, "identity": binding_str(binding) })))
}

/// 身份路由:/identity、/identity/import、/planets/{id}/identity
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/identity", get(identity_list))
        .route("/identity/import", post(identity_import))
        .route("/planets/{id}/identity", put(planet_identity_set))
}
```

注:若 `runtime_of` 不在 `crate::state` 或签名不同,以 `crates/daemon/src/api_planet.rs` 中 DELETE/stop 端点的实际防御写法为准对齐;`find_entry`( api_planet.rs:110 )支持 id/name/worldId 解析,如需名称寻址可复用,但 PUT 路径参数语义与 `planet_action` 一致用 canonical id 即可。

- [ ] **Step 4: main.rs 挂载**

`mod api_net;` 之后加 `mod api_identity;`;router 构建处( `:535-542` )在 `.merge(api_planet::router())` 之后加 `.merge(api_identity::router())`。

- [ ] **Step 5: 构建验证**

Run: `cargo build --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 通过。手动冒烟(可选,daemon 已在本机运行时):`curl -s -H "X-ZT1-Auth: $(sudo cat <config>/authtoken.secret)" http://127.0.0.1:<port>/identity` 返回 `{"global":{"exists":false,...},"planets":[...]}`。

- [ ] **Step 6: Commit**

```bash
git add crates/daemon/src/api_identity.rs crates/daemon/src/main.rs crates/api/src/endpoints.rs crates/api/src/types.rs
git commit -m "feat(daemon): 身份查询/导入/切换绑定 REST 端点

GET /identity 输出全局身份与各 planet 绑定及节点地址;POST
/identity/import 校验官方 identity 双文件后停禁官方服务再落盘
config 根,失败即中止防同身份双开;PUT /planets/{id}/identity 切换
绑定,运行中拒绝。"
```

---

### Task 5: CLI identity 命令

**Files:**
- Modify: `crates/cli/src/client.rs:211-214`(put_json)
- Create: `crates/cli/src/cmd/identity.rs`
- Modify: `crates/cli/src/cmd/mod.rs`(pub mod identity)
- Modify: `crates/cli/src/main.rs`(Command 枚举 + 分发)
- Modify: `crates/cli/locales/zh-Hans.yml` / `en.yml` / `zh-Hant.yml`

**Interfaces:**
- Consumes: Task 4 的 REST 契约与 `planet_port_api::endpoints::{identity, identity_import, planet_identity}`。
- Produces: `planet-port identity [list]` / `identity import [--path <p>] [--force]` / `identity use <planet> <global|own>`。

- [ ] **Step 1: client.rs 加 put_json**

`patch_json`( `:211` )之后镜像加:

```rust
    pub fn put_json(&self, path: &str, body: &Value) -> Result<Value, CliError> {
        let (c, t) = self.request("PUT", path, Some(body))?;
        self.parse(c, t)
    }
```

- [ ] **Step 2: 新建 cmd/identity.rs**

```rust
//! identity 子命令:身份模式与身份列表(缺省)、官方身份导入、切换绑定。

use clap::{Subcommand, ValueEnum};
use serde_json::json;

use crate::client::{CliError, Client};
use planet_port_api::endpoints;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum IdentityMode {
    Own,
    Global,
}

impl IdentityMode {
    fn as_str(&self) -> &'static str {
        match self {
            IdentityMode::Own => "own",
            IdentityMode::Global => "global",
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum IdentityCmd {
    /// List identity mode and identities (default)
    List,
    /// Import official ZeroTier identity as global identity
    Import {
        /// Path to official identity.secret (or its dir); omit to auto-detect
        #[arg(long)]
        path: Option<String>,
        /// Overwrite existing global identity
        #[arg(long)]
        force: bool,
    },
    /// Switch a planet's identity binding
    Use {
        planet: String,
        mode: IdentityMode,
    },
}

pub fn run(client: &Client, cmd: &Option<IdentityCmd>, json: bool) -> Result<(), CliError> {
    match cmd.as_deref().unwrap_or(&IdentityCmd::List) {
        IdentityCmd::List => list(client, json),
        IdentityCmd::Import { path, force } => import(client, path.as_deref(), *force, json),
        IdentityCmd::Use { planet, mode } => switch(client, planet, *mode, json),
    }
}

/// /identity 的 planets 段里按 id 或 name(忽略大小写)找 canonical id
fn resolve_id(client: &Client, planet: &str) -> Result<String, CliError> {
    let v = client.get_json(endpoints::identity())?;
    for p in v["planets"].as_array().unwrap_or(&vec![]) {
        if p["id"].as_str() == Some(planet)
            || p["name"].as_str().map(|n| n.eq_ignore_ascii_case(planet)) == Some(true)
        {
            return Ok(p["id"].as_str().unwrap_or(planet).to_string());
        }
    }
    // 未命中也透传,由 daemon 404 planetNotFound 统一报错
    Ok(planet.to_string())
}

fn list(client: &Client, json: bool) -> Result<(), CliError> {
    let v = client.get_json(endpoints::identity())?;
    if json {
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return Ok(());
    }
    let g = &v["global"];
    match g["nodeId"].as_str() {
        Some(node) => println!("{}", rust_i18n::t!("cli.identity.global", node = node)),
        None => println!("{}", rust_i18n::t!("cli.identity.globalNone")),
    }
    for p in v["planets"].as_array().unwrap_or(&Vec::new()) {
        let node = p["nodeId"].as_str().unwrap_or("-");
        println!(
            "{}  {}  {}  {}",
            p["name"].as_str().unwrap_or("-"),
            p["identity"].as_str().unwrap_or("-"),
            node,
            if p["running"].as_bool().unwrap_or(false) { "running" } else { "stopped" },
        );
    }
    Ok(())
}

fn import(client: &Client, path: Option<&str>, force: bool, json: bool) -> Result<(), CliError> {
    let mut body = json!({});
    if let Some(p) = path {
        body["path"] = json!(p);
    }
    if force {
        body["force"] = json!(true);
    }
    let resp = client.post_json(endpoints::identity_import(), &body)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&resp).unwrap_or_default());
        return Ok(());
    }
    let node = resp["nodeId"].as_str().unwrap_or("-");
    match resp["serviceAction"].as_str() {
        Some("stoppedAndDisabled") => println!(
            "{}",
            rust_i18n::t!("cli.identity.importedStopped", node = node)
        ),
        Some("notInstalled") => {
            println!("{}", rust_i18n::t!("cli.identity.imported", node = node));
            println!("{}", rust_i18n::t!("cli.identity.officialAbsent"));
        }
        _ => println!("{}", rust_i18n::t!("cli.identity.imported", node = node)),
    }
    Ok(())
}

fn switch(client: &Client, planet: &str, mode: IdentityMode, json: bool) -> Result<(), CliError> {
    let id = resolve_id(client, planet)?;
    if mode == IdentityMode::Own {
        // 独立身份 = 新节点地址(或复用留盘旧 own 身份),成员资格需重新授权
        eprintln!("{}", rust_i18n::t!("cli.identity.switchOwnWarn"));
    }
    let body = json!({ "identity": mode.as_str() });
    let resp = client.put_json(&endpoints::planet_identity(&id), &body)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&resp).unwrap_or_default());
        return Ok(());
    }
    println!(
        "{}",
        rust_i18n::t!("cli.identity.switched", planet = planet, mode = mode.as_str())
    );
    Ok(())
}
```

- [ ] **Step 3: cmd/mod.rs 与 main.rs 接线**

`crates/cli/src/cmd/mod.rs` 加 `pub mod identity;`。

`crates/cli/src/main.rs` 的 `Command` 枚举 `Planet { ... }` 变体之后加:

```rust
    /// Identity mode, identity list, official identity import, binding switch
    Identity {
        #[command(subcommand)]
        cmd: Option<cmd::identity::IdentityCmd>,
    },
```

分发处(`main.rs` 尾部 match)加:

```rust
    Command::Identity { cmd } => cmd::identity::run(&client, &cmd, json)?,
```

(分发变量名以现有代码为准,如 `cli.json` 已解构则用解构后的值。)

- [ ] **Step 4: locales 三语文案**

`crates/cli/locales/zh-Hans.yml` 在 `# ---- 命令反馈 ----` 节内加:

```yaml
  # ---- identity ----
  cli.identity.global: "全局身份: %{node}"
  cli.identity.globalNone: 全局身份: (未建立,可 identity import 导入官方身份)
  cli.identity.imported: "已导入全局身份: %{node}"
  cli.identity.importedStopped: "已导入全局身份: %{node}(官方服务已停止并禁用自启)"
  cli.identity.officialAbsent: 本机未检测到官方 ZeroTier 客户端(身份文件可能来自其它机器)
  cli.identity.switchOwnWarn: "注意: 切到独立身份后该 planet 将使用新节点地址,原有网络成员资格需重新授权"
  cli.identity.switched: "planet %{planet} 身份绑定已切换为 %{mode}(下次启动生效)"
```

`en.yml` 对应英文:

```yaml
  # ---- identity ----
  cli.identity.global: "global identity: %{node}"
  cli.identity.globalNone: "global identity: (none yet, run 'identity import' to import the official one)"
  cli.identity.imported: "global identity imported: %{node}"
  cli.identity.importedStopped: "global identity imported: %{node} (official service stopped & disabled)"
  cli.identity.officialAbsent: official ZeroTier client not detected on this machine (identity file may come from another machine)
  cli.identity.switchOwnWarn: "note: switching to own identity changes this planet's node address; network memberships need re-authorization"
  cli.identity.switched: "planet %{planet} identity binding switched to %{mode} (effective on next start)"
```

`zh-Hant.yml` 对应繁体(「全局身份」「已導入」「注意」「切換」「節點地址」「重新授權」等,风格对齐该文件现有词条)。

- [ ] **Step 5: 构建验证**

Run: `cargo build --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 通过。`cargo run -p planet-port -- identity --help` 能列出 list/import/use。

- [ ] **Step 6: Commit**

```bash
git add crates/cli/src/client.rs crates/cli/src/cmd/identity.rs crates/cli/src/cmd/mod.rs crates/cli/src/main.rs crates/cli/locales
git commit -m "feat(cli): 新增 identity 命令

缺省列出全局身份与各 planet 绑定;import 自动探测/显式路径导入
官方身份并按响应提示官方服务处置结果;use 切换 planet 身份绑定,
切 own 时警告节点地址将变化需重新授权。"
```

---

### Task 6: 交叉编译验证与 README 更新

**Files:**
- Modify: `README.md`(功能/命令清单各加一条)

**Interfaces:** 无代码接口;交付验证。

- [ ] **Step 1: README 补条目**

在 README 的命令或功能清单(以文件实际结构为准)加一条,如:

```markdown
- `planet-port identity` — 身份模式与身份列表;`identity import` 导入官方 ZeroTier 身份(自动停禁官方服务);`identity use <planet> global|own` 切换绑定
```

- [ ] **Step 2: 全平台构建验证**

Run:
```bash
cargo build --workspace && cargo clippy --workspace --all-targets -- -D warnings
cargo zigbuild --target x86_64-unknown-linux-gnu -p planet-portd   # 按项目现有 zigbuild 目标与 profile,见 scripts/ 或 docs/install.md
cargo zigbuild --target x86_64-pc-windows-gnu -p planet-portd      # 同上,以现有交叉矩阵为准
```
Expected: 全部通过(具体 target 列表以仓库现有交叉编译配置为准,若 scripts/ 有现成脚本直接跑脚本)。

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: README 补 identity 命令说明"
```

---

## 附:真机 e2e 清单(macOS,由用户主导,不在 subagent 执行范围)

1. 本机装有官方 ZeroTier 客户端且已入网;`sudo planet-port identity import`
2. 预期:输出已导入+官方服务已停止禁用;`launchctl print system/com.zerotier.one` 确认 disabled;`/Library/Application Support/ZeroTier/identity.secret` 与 `<config>/identity.secret` 地址一致
3. 新导入/新建 planet 绑定 global,`planet start` 后 `planet-port identity` 列表 node id = 官方地址,原网络成员资格无需重新授权即恢复
4. `identity use <planet> own` → start → node id 变化;`identity use <planet> global` → start → 恢复全局地址(dataDir 旧 own 身份仍留盘)
5. `identity import` 未 force 二次导入 → 409 identityExists
