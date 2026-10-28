# daemon 一期 POC(1-3)实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 验证 Rust 内嵌 libzerotiercore 1.16.2 的三个高风险假设(FFI 控制面、feth L2 设备数据面、单进程双 Node),产出 daemon 地基 crate(zt-sys/zt-core)。

**Architecture:** bootstrap 脚本拉官方 tarball 裁剪子集到不入库的 Core/,zt-sys(cc 编译 + bindgen),zt-core(每 Node 一条驱动线程串行调用全部 process*,回调经 uptr 还原 Rust 对象,planet 经 stateGet 注入),pocs 三个 bin。

**Tech Stack:** Rust、cc/bindgen(build.rs)、tun-rs 2.8(L2 TAP/feth)、route_manager 0.2、nix(poll/pipe)、clap(pocs bin)。

**前置事实(已核实,实现时不要重新发明)**,详见 spec `docs/superpowers/specs/2026-10-28-daemon-poc.md` §3 与本次调查:
- C API 全集 23 个函数、8 回调(6 必填),`callbacks.version=0`,`ZT_Node_Config` 指针**必须非 NULL**(全零即可)
- 驱动:`ZT_Node_processBackgroundTasks(now_ms, &deadline)` 到点才调,deadline 为下次绝对时刻(下限 60ms);`ZT_Node_delete` 前必须停其它线程;对同一 Node 的 process* 必须串行
- planet 注入:stateGet 命中 `(ZT_STATE_OBJECT_PLANET, id={0,0})` 返回 World 二进制;put 会回写,落盘;无 stateDeleteFunction(删除=put len=-1)
- ARP/组播硬性要求:对本机每个 IPv4 调 `multicastSubscribe(nwid, 0xffffffffffff, adi=IPv4主机序)`,每 5000ms 扫描 diff
- macOS feth L2 = 内核 ARP;utun L3 才需要用户态 ARP 垫层(退路)
- 时间源:毫秒墙钟(`SystemTime::UNIX_EPOCH` elapsed.as_millis)
- sockaddr 的 prefix 编码在 `sin_port` 字段(assignedAddresses/routes 的 netmask 语义)
- 测试环境:planet `pocs/fixtures/planet.Gozi25`(worldId 3530253632),网络 `5cf54f181554090f`,本机 zerotier-one 占 9993,POC 用 9994/9995;join 后需用户在控制器授权新设备;POC 2/3 的 sudo 运行由用户执行

## Global Constraints

- 核心源码**不入库**:`Core/` 整目录与 `crates/pocs/fixtures/` 在 .gitignore;bootstrap 钉 `ZT_VERSION=1.16.2` + `ZT_SHA256=2c607f573c6e38815433af289d364a689a203b18b51125f06c4472014d0657f0`(与 iOS/Android 同值),构建期校验 `Core/.fetched-tag` 不匹配即 panic
- 编译核心:仅 `node/*.cpp + osdep/OSUtils.cpp`,C++17,宏 `ZT_NO_PEER_METRICS`,**不得定义 `ZT_SDK`**;cc crate 驱动(不依赖宿主 cmake)
- 不引入测试框架,不加测试用例;验证 = 每个 POC bin 的可观察行为
- 不修改已完成的 crates/{api,mockd,cli} 与既有文档
- git 一律 `--no-pager`;提交信息 Conventional Commits 中文 + Body,禁止任何 Co-authored-by/AI 署名
- 新增依赖许可:MIT/Apache 优先(tun-rs Apache-2.0、route_manager Apache-2.0、nix MIT、cc MIT/Apache、bindgen BSD-3——记录即可)
- 所有 FFI 边界:回调内禁 panic(包 `catch_unwind` 或确保无 unwrap)、禁跨越 FFI 存活的 Rust 引用;`ZT_Node_delete` 前必须 join 完驱动线程与设备线程

---

### Task 1: bootstrap 拉源 + zt-sys 静态库与 bindgen

**Files:**
- Create: `scripts/bootstrap.sh`(可执行)
- Create: `crates/zt-sys/Cargo.toml`、`crates/zt-sys/build.rs`、`crates/zt-sys/src/lib.rs`
- Modify: `Cargo.toml`(workspace members 加 `"crates/zt-sys"`)、`.gitignore`

**Interfaces(后续任务依赖):**
- Produces: crate `zt-sys`,`pub use generated bindings as zt::*`(全部 ZT_* 类型/函数/常量,`#[allow(non_camel_case_types, non_snake_case, non_upper_case_globals, dead_code)]`)

- [ ] **Step 1: bootstrap.sh**

对齐 iOS/Android 模板,裁剪去掉 java/jni(BSD 上 `shasum` 在 macOS 自带):

```bash
#!/usr/bin/env bash
# 拉取 ZeroTier 核心源码子集(不入库)。与 iOS/Android 端同一套模板:
# 钉 tag + SHA256 拉官方 tarball,解压裁剪到 Core/,写 .fetched-tag 版本戳。
set -euo pipefail

ZT_VERSION=1.16.2
ZT_SHA256=2c607f573c6e38815433af289d364a689a203b18b51125f06c4472014d0657f0
ZT_URL="https://codeload.github.com/zerotier/ZeroTierOne/tar.gz/${ZT_VERSION}"

CACHE_DIR="${ZT_CACHE_DIR:-$HOME/Library/Caches/PlanetPort}"
TARBALL="${CACHE_DIR}/ZeroTierOne-${ZT_VERSION}.tar.gz"
CORE_DIR=Core
STAMP="${CORE_DIR}/.fetched-tag"

err() { echo "bootstrap: $*" >&2; exit 1; }

if [[ -f "$STAMP" && "$(cat "$STAMP")" == "$ZT_VERSION" ]]; then
  echo "ZeroTier 核心 ${ZT_VERSION} 已就绪，跳过拉取"
  exit 0
fi

verify_tarball() { echo "${ZT_SHA256}  ${TARBALL}" | shasum -a 256 --check - >/dev/null 2>&1; }

mkdir -p "$CACHE_DIR"
if [[ -s "$TARBALL" ]] && ! verify_tarball; then
  err "缓存损坏，已删除：${TARBALL}" ; rm -f "$TARBALL"
fi
if [[ ! -s "$TARBALL" ]]; then
  echo "下载 ${ZT_TARBALL_URL:-$ZT_URL}"
  curl -fL --retry 3 -o "$TARBALL" "${ZT_TARBALL_URL:-$ZT_URL}" || err "下载失败"
fi
verify_tarball || err "SHA256 校验失败（可能被镜像篡改或下载不完整）：${TARBALL}"

TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
tar -xzf "$TARBALL" -C "$TMP"
SRC="${TMP}/ZeroTierOne-${ZT_VERSION}"

# 只取 POC 所需子集：node 整目录、C API 头、OSUtils、ext 头与汇编。
# 比 Android 端少 Binder/BlockingQueue/ExtOsdep/Phy(那是 JNI/service 层需要的)。
rm -rf "$CORE_DIR"; mkdir -p "$CORE_DIR"
cp -R "$SRC/node" "$CORE_DIR/"
mkdir -p "$CORE_DIR/include" "$CORE_DIR/osdep"
cp "$SRC/include/ZeroTierOne.h" "$SRC/include/ZeroTierDebug.h" "$CORE_DIR/include/"
cp "$SRC/osdep/OSUtils.cpp" "$SRC/osdep/OSUtils.hpp" "$CORE_DIR/osdep/"
cp "$SRC/version.h" "$CORE_DIR/"
mkdir -p "$CORE_DIR/ext/nlohmann" \
  "$CORE_DIR/ext/prometheus-cpp-lite-1.0/core/include" \
  "$CORE_DIR/ext/prometheus-cpp-lite-1.0/simpleapi/include"
cp "$SRC/ext/nlohmann/json.hpp" "$CORE_DIR/ext/nlohmann/"
cp -R "$SRC/ext/prometheus-cpp-lite-1.0/core/include/prometheus" "$CORE_DIR/ext/prometheus-cpp-lite-1.0/core/include/"
cp -R "$SRC/ext/prometheus-cpp-lite-1.0/simpleapi/include/prometheus" "$CORE_DIR/ext/prometheus-cpp-lite-1.0/simpleapi/include/"
cp -R "$SRC/ext/arm32-neon-salsa2012-asm" "$SRC/ext/x64-salsa2012-asm" "$CORE_DIR/ext/"

echo "$ZT_VERSION" > "$STAMP"
echo "ZeroTier 核心 ${ZT_VERSION} 就绪 → ${CORE_DIR}/"
```

- [ ] **Step 2: .gitignore 追加**

```
# ZeroTier 核心源码(bootstrap 产物,不入库)
/Core/
# POC 本地环境:planet 副本与运行期 dataStore
crates/pocs/fixtures/
```

- [ ] **Step 3: zt-sys crate**

`crates/zt-sys/Cargo.toml`:

```toml
[package]
name = "zt-sys"
version = "0.1.0"
edition = "2021"
build = "build.rs"
links = "zerotiercore"
license = "MPL-2.0"

# 依赖许可:cc MIT/Apache-2.0;bindgen BSD-3-Clause(传递 clang-sys Apache-2.0/LLVM)
[build-dependencies]
cc = "1"
bindgen = { version = "0.70", default-features = false, features = ["runtime"] }
```

`crates/zt-sys/build.rs`:

```rust
// 编译 libzerotiercore 静态库并生成 bindgen 绑定。
// 源码由 scripts/bootstrap.sh 拉取(不入库);此处校验版本戳防错版编译。
use std::{fs, path::PathBuf};

const ZT_VERSION: &str = "1.16.2";

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.parent().parent().unwrap().to_path_buf();
    let core = root.join("Core");
    let stamp = core.join(".fetched-tag");

    match fs::read_to_string(&stamp) {
        Ok(v) if v.trim() == ZT_VERSION => {}
        _ => panic!(
            "ZeroTier 核心 {ZT_VERSION} 未就绪:先执行 ./scripts/bootstrap.sh(见 Core/.fetched-tag)"
        ),
    }

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .define("ZT_NO_PEER_METRICS", None)
        .warnings(false)
        .include(core.join("include"))
        .include(core.join("node"))
        .include(core.join("osdep"))
        .include(core.join("ext"))
        .include(core.join("ext/prometheus-cpp-lite-1.0/core/include"))
        .include(core.join("ext/prometheus-cpp-lite-1.0/simpleapi/include"))
        .file(core.join("osdep/OSUtils.cpp"));
    for e in fs::read_dir(core.join("node")).unwrap() {
        let p = e.unwrap().path();
        if p.extension().and_then(|s| s.to_str()) == Some("cpp") {
            build.file(p);
        }
    }
    build.compile("zerotiercore");

    let bindings = bindgen::Builder::default()
        .header(core.join("include/ZeroTierOne.h").to_str().unwrap())
        .layout_tests(false)
        .generate_comments(false)
        .generate()
        .expect("bindgen ZeroTierOne.h 失败");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out.join("bindings.rs"))
        .expect("写 bindings.rs 失败");

    println!("cargo:rerun-if-changed={}", core.join(".fetched-tag").display());
}
```

`crates/zt-sys/src/lib.rs`:

```rust
//! ZeroTier 核心 C API 绑定(bindgen 生成,1.16.2)。
#![allow(non_camel_case_types, non_snake_case, non_upper_case_globals, dead_code)]
include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
```

- [ ] **Step 4: 验证**

```bash
chmod +x scripts/bootstrap.sh && ./scripts/bootstrap.sh
# 预期:下载 + SHA256 通过 + "就绪";二次运行输出 "已就绪，跳过拉取"
git --no-pager status --short   # Core/ 不出现在 untracked(被 ignore)
cargo build -p zt-sys           # 预期:编译 80 个 C++ 文件成功
cargo build -p zt-sys 2>&1 | grep -c "ZeroTier 核心" || true
# 交叉验证绑定产物:target 下找 bindings.rs,确认含 ZT_Node_new/ZT_STATE_OBJECT_PLANET
find target -name bindings.rs -exec grep -l "ZT_Node_new" {} \;
```

- [ ] **Step 5: 提交**

```bash
git --no-pager add scripts/bootstrap.sh .gitignore Cargo.toml crates/zt-sys
git commit -m "build: bootstrap 拉取核心源码并产出 zt-sys 绑定

对齐 iOS/Android 模板:钉 1.16.2 tag + SHA256 拉官方 tarball,
裁剪 node/osdep/ext 子集到不入库的 Core/;build.rs 校验版本戳
后以 cc 编译静态库(C++17,ZT_NO_PEER_METRICS),bindgen 于
ZeroTierOne.h 生成绑定。"
```

---

### Task 2: zt-core 状态存储、回调 glue 与 Node 生命周期

**Files:**
- Create: `crates/zt-core/Cargo.toml`、`crates/zt-core/src/lib.rs`、`crates/zt-core/src/state.rs`、`crates/zt-core/src/sa.rs`、`crates/zt-core/src/node.rs`、`crates/zt-core/src/config.rs`

**Interfaces:**
- Consumes: `zt-sys` 全部绑定
- Produces(Task 3-5 依赖,签名精确):
  - `pub struct NodeParams { pub data_dir: PathBuf, pub planet_file: Option<PathBuf>, pub bind_port: u16 }`
  - `pub trait NodeSink: Send + 'static { fn on_event(&mut self, ev: NodeEvent); fn on_net_config(&mut self, nwid: u64, op: NetConfigOp, cfg: NetConfig); fn on_frame(&mut self, nwid: u64, src_mac: u64, dst_mac: u64, ether_type: u16, payload: &[u8]); fn local_ipv4s(&self) -> Vec<std::net::Ipv4Addr>; }`
  - `pub enum NodeEvent { Up, Online, Offline, Down, IdentityCollision, Fatal(String) }`
  - `pub enum NetConfigOp { Up, ConfigUpdate, Down, Destroy }`
  - `pub struct NetConfig { pub nwid: u64, pub name: String, pub mac: u64, pub status: NetStatus, pub mtu: u16, pub netconf_revision: u64, pub assigned: Vec<(IpNet)>, pub routes: Vec<ManagedRoute>, pub dns_domain: Option<String>, pub dns_servers: Vec<IpAddr> }`
  - `pub struct IpNet { pub addr: IpAddr, pub prefix: u8 }`;`pub struct ManagedRoute { pub target: IpNet, pub via: Option<IpAddr>, pub metric: u16 }`
  - `pub enum NetStatus { RequestingConfiguration, Ok, AccessDenied, NotFound, PortError, ClientTooOld, AuthenticationRequired }`
  - `impl NodeHandle { pub fn start(params: NodeParams, sink: impl NodeSink) -> Result<Self, NodeError>; pub fn address(&self) -> u64; pub fn core_version() -> (u32,u32,u32); pub fn shutdown(self); }`(join/leave/status/networks 在 Task 3 加)
  - `pub enum NodeError { DataStoreFailed(String), InvalidArg(String), Io(std::io::Error) }`

- [ ] **Step 1: Cargo.toml 与 lib.rs**

```toml
[package]
name = "zt-core"
version = "0.1.0"
edition = "2021"
license = "MPL-2.0"

[dependencies]
zt-sys = { path = "../zt-sys" }
libc = "0.2"
```

`src/lib.rs`:

```rust
//! libzerotiercore 的 safe 封装:Node 生命周期、dataStore、回调 glue。
pub mod config;
pub mod node;
pub mod sa;
pub mod state;

pub use config::{IpNet, ManagedRoute, NetConfig, NetConfigOp, NetStatus, NodeEvent, NodeParams, NodeSink};
pub use node::{NodeError, NodeHandle};
```

- [ ] **Step 2: sa.rs(sockaddr 转换,prefix 编码在 port 字段)**

```rust
//! sockaddr_storage <-> SocketAddr;ZeroTier 把前缀长度编码在 sin_port。
use std::net::{IpAddr, SocketAddr};
use std::os::unix::ffi::OsStrExt;

pub fn from_sockaddr_storage(ss: *const libc::sockaddr_storage) -> Option<SocketAddr> {
    unsafe {
        let family = (*(ss as *const libc::sockaddr)).sa_family as i32;
        if family == libc::AF_INET {
            let a = &*(ss as *const libc::sockaddr_in);
            Some(SocketAddr::new(
                std::net::Ipv4Addr::from(a.sin_addr.s_addr.to_be_bytes()),
                u16::from_be(a.sin_port),
            ))
        } else if family == libc::AF_INET6 {
            let a = &*(ss as *const libc::sockaddr_in6);
            Some(SocketAddr::new(
                std::net::Ipv6Addr::from(a.sin6_addr.s6_addr),
                u16::from_be(a.sin6_port),
            ))
        } else {
            None
        }
    }
}

/// SocketAddr(或带 prefix 的网络条目)写回 sockaddr_storage。
/// `port` 参数:收发物理包用真实端口;网络条目用它承载前缀长度。
pub fn to_sockaddr_storage(addr: &SocketAddr, port_override: Option<u16>) -> libc::sockaddr_storage {
    let mut ss: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let port = port_override.unwrap_or(addr.port()).to_be();
    match addr.ip() {
        IpAddr::V4(v4) => unsafe {
            let a = &mut *(&mut ss as *mut _ as *mut libc::sockaddr_in);
            a.sin_family = libc::AF_INET as _;
            a.sin_port = port;
            a.sin_addr.s_addr = u32::from_be_bytes(v4.octets());
        },
        IpAddr::V6(v6) => unsafe {
            let a = &mut *(&mut ss as *mut _ as *mut libc::sockaddr_in6);
            a.sin6_family = libc::AF_INET6 as _;
            a.sin6_port = port;
            a.sin6_addr.s6_addr = v6.octets();
        },
    }
    ss
}
```

(实现者注意:`std::os::unix::ffi::OsStrExt` 此处未用,删掉该 import;sa_family 在 Darwin 位于第二字节,经 libc::sockaddr 读取即可。)

- [ ] **Step 3: state.rs(dataStore 语义,平移 Android stateFile)**

```rust
//! dataStore 对象 ↔ 落盘文件。语义平移 Android NodeRuntime.stateFile:
//! identity 跨 planet 共享于根;moon/peer/netconf 按 planet worldId 分目录;
//! planet 重定向到注入文件(get)/根下 planet(put 回写)。
use std::fs;
use std::io;
use std::path::PathBuf;

pub struct StateStore {
    root: PathBuf,
    world_dir: PathBuf,
    planet_in: Option<PathBuf>,
}

impl StateStore {
    pub fn new(root: PathBuf, world_id: u64, planet_in: Option<PathBuf>) -> Self {
        let world_dir = root.join(world_id.to_string());
        Self { root, world_dir, planet_in }
    }

    /// 返回 None = 未知对象(get/put 各按错误码上报)。
    fn file_for(&self, obj: zt_obj, id: &[u64; 2]) -> Option<PathBuf> {
        use zt_obj::*;
        match obj {
            IdentityPublic => Some(self.root.join("identity.public")),
            IdentitySecret => Some(self.root.join("identity.secret")),
            PlanetGet => self.planet_in.clone(),
            PlanetPut => Some(self.root.join("planet")),
            Moon => Some(self.world_dir.join(format!("moons.d/{:016x}.moon", id[0]))),
            Peer => Some(self.world_dir.join(format!("peers.d/{:010x}.peer", id[0]))),
            NetworkConfig => Some(self.world_dir.join(format!("networks.d/{:016x}.conf", id[0]))),
        }
    }

    /// 返回字节数;未找到 -1;IO 错误 -2。buffer 为 NULL 时仅探测大小。
    pub fn get(&self, obj: zt_obj, id: &[u64; 2], buf: *mut u8, cap: usize) -> isize {
        let Some(f) = self.file_for(obj, id) else { return -2 };
        let data = match fs::read(&f) { Ok(d) => d, Err(e) if e.kind() == io::ErrorKind::NotFound => return -1, Err(_) => return -2 };
        if buf.is_null() { return data.len() as isize; }
        let n = data.len().min(cap);
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), buf, n); }
        n as isize
    }

    /// len == -1 表示删除。返回 0 成功,-1 失败。
    pub fn put(&self, obj: zt_obj, id: &[u64; 2], data: *const u8, len: isize, secure: bool) -> isize {
        let Some(f) = self.file_for(obj, id) else { return -1 };
        if len == -1 { let _ = fs::remove_file(&f); return 0; }
        if data.is_null() { return -1; }
        let bytes = unsafe { std::slice::from_raw_parts(data, len as usize) };
        if let Some(p) = f.parent() { let _ = fs::create_dir_all(p); }
        let tmp = f.with_extension("tmp");
        if fs::write(&tmp, bytes).is_err() { return -1; }
        if fs::rename(&tmp, &f).is_err() { let _ = fs::remove_file(&tmp); return -1; }
        // identity.secret(与 secure 标记)收紧为 0600
        if secure { let _ = fs::set_permissions(&f, <fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o600)); }
        0
    }
}

/// 把 C 枚举收敛为 store 内部对象种类(node.rs 的 glue 里做数值映射)。
#[derive(Clone, Copy)]
pub enum zt_obj { IdentityPublic, IdentitySecret, PlanetGet, PlanetPut, Moon, Peer, NetworkConfig }
```

(实现者注意:`fs::Permissions` import 写法按编译器提示修正;0600 收紧逻辑参照 Android:`secure || name == identity.secret`——PLANET/IDENTITY_SECRET 的 put 传 secure=true。)

- [ ] **Step 4: config.rs(对外类型 + ZT 结构体拷贝)**

完整定义 `NodeParams/NodeSink/NodeEvent/NetConfigOp/NetStatus/IpNet/ManagedRoute/NetConfig` 如 Interfaces 所列;另提供:

```rust
impl NetConfig {
    /// 从 ZT_VirtualNetworkConfig 拷贝(回调内指针不保证存活,必须立即拷)。
    pub unsafe fn from_zt(c: &zt_sys::ZT_VirtualNetworkConfig) -> Self {
        // assignedAddresses[i]: sockaddr,port 字段 = 前缀长度
        // routes[i].target 同上;via 可为全零(None)
        // dns.domain 为空即 None;dns.server_addr 有效条目收集
        // name: CStr::from_ptr 直至 NUL
        // status/netconfRevision/mtu/mac/nwid 直拷
        todo_in_plan!(由实现者按上述逐字段写出,映射:ZT_VirtualNetworkStatus 0..6 → NetStatus;ZT_VirtualNetworkConfigOperation 1..4 → NetConfigOp)
    }
}
```

**计划内唯一授权的"按注释实现"点**:from_zt 是逐字段机械映射,映射规则如上注释(实现者写字段级代码,不得改动语义);其余代码一律照抄本计划。

- [ ] **Step 5: node.rs(回调 glue + 生命周期,本任务先不含驱动线程)**

```rust
//! NodeHandle:uptr 还原、6 必填回调 glue、new/delete。
//! 本任务只做"构造 → identity 生成 → 析构";驱动线程与 UDP 在 Task 3。
use crate::{config::*, sa::*, state::StateStore};
use std::net::UdpSocket;
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use zt_sys as zt;

pub enum NodeError { DataStoreFailed(String), InvalidArg(String), Io(std::io::Error) }

struct Inner {
    store: StateStore,
    udp: UdpSocket,
    // 以下通道由 Task 3 的驱动线程消费;本任务先建好后由 shutdown 关闭
    frame_out: Sender<FrameOut>,   // 核心发往虚拟网络的帧(POC1 丢弃)
    pub(crate) running: AtomicBool,
}

pub(crate) enum FrameOut { Vnet { nwid: u64, src: u64, dst: u64, eth: u16, payload: Vec<u8> } }

unsafe impl Send for NodeHandle {}
pub struct NodeHandle {
    node: *mut zt::ZT_Node,
    inner: *mut Inner,
}

impl NodeHandle {
    pub fn start(params: NodeParams, sink: impl NodeSink) -> Result<Self, NodeError> {
        std::fs::create_dir_all(&params.data_dir).map_err(NodeError::Io)?;
        let planet = std::fs::read(params.planet_file.as_ref().ok_or(NodeError::InvalidArg("planet_file 必填".into()))?).map_err(NodeError::Io)?;
        let world_id = u64::from_be_bytes(planet[1..9].try_into().unwrap());
        let udp = UdpSocket::bind(("0.0.0.0", params.bind_port)).map_err(NodeError::Io)?;
        udp.set_nonblocking(true).map_err(NodeError::Io)?;
        let inner = Box::into_raw(Box::new(Inner {
            store: StateStore::new(params.data_dir.clone(), world_id, params.planet_file.clone()),
            udp, frame_out: /* 建 channel 存 sender,receiver 交驱动线程 */,
            running: AtomicBool::new(true),
        }));
        let cbs = zt::ZT_Node_Callbacks {
            version: 0,
            statePutFunction: Some(state_put),
            stateGetFunction: Some(state_get),
            wirePacketSendFunction: Some(wire_send),
            virtualNetworkFrameFunction: Some(vnet_frame),
            virtualNetworkConfigFunction: Some(vnet_config),
            eventCallback: Some(on_event),
            pathCheckFunction: None,
            pathLookupFunction: None,
        };
        let cfg = zt::ZT_Node_Config { enable_encrypted_hello: 0, low_bandwidth_mode: 0 };
        let now = now_ms();
        let mut node: *mut zt::ZT_Node = std::ptr::null_mut();
        let rc = zt::ZT_Node_new(&mut node, &cfg, inner as *mut _, std::ptr::null_mut(), &cbs, now as _);
        if rc != zt::ZT_ResultCode_ZT_RESULT_OK {
            // 清理 inner;按 rc 映射错误(FATAL_DATA_STORE→DataStoreFailed)
            ...
        }
        // 启动即驱动一次:identity 生成/装载、planet 读入在此发生(对齐 Android)
        let mut dl: i64 = 0;
        let rc = zt::ZT_Node_processBackgroundTasks(node, std::ptr::null_mut(), now_ms() as _, &mut dl);
        ...
        Ok(Self { node, inner })
    }

    pub fn address(&self) -> u64 { unsafe { zt::ZT_Node_address(self.node) } }
    pub fn core_version() -> (u32, u32, u32) { let (mut a, mut b, mut c) = (0, 0, 0); unsafe { zt::ZT_version(&mut a, &mut b, &mut c) }; (a as _, b as _, c as _) }

    pub fn shutdown(self) {
        unsafe {
            self.inner().running.store(false, Ordering::SeqCst);
            // Task 3 起:先 join 驱动/设备线程,再 delete
            zt::ZT_Node_delete(self.node);
            drop(Box::from_raw(self.inner));
        }
    }
    fn inner(&self) -> &Inner { unsafe { &*self.inner } }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

// ===== 6 个回调 glue:uptr 还原 Inner,内部禁 panic/禁 unwrap =====

unsafe extern "C" fn state_get(_n: *mut zt::ZT_Node, uptr: *mut core::ffi::c_void, _t: *mut core::ffi::c_void,
    obj: zt::ZT_StateObjectType, id: *const u64, buf: *mut core::ffi::c_void, len: core::ffi::c_uint) -> core::ffi::c_int {
    let inner = &*(uptr as *const Inner);
    let map = |o: zt::ZT_StateObjectType| -> Option<crate::state::zt_obj> { use crate::state::zt_obj::*; use zt::ZT_StateObjectType as S;
        match o { S::ZT_STATE_OBJECT_IDENTITY_PUBLIC => Some(IdentityPublic), S::ZT_STATE_OBJECT_IDENTITY_SECRET => Some(IdentitySecret),
            S::ZT_STATE_OBJECT_PLANET => Some(PlanetGet), S::ZT_STATE_OBJECT_MOON => Some(Moon),
            S::ZT_STATE_OBJECT_PEER => Some(Peer), S::ZT_STATE_OBJECT_NETWORK_CONFIG => Some(NetworkConfig), _ => None } };
    let Some(o) = map(obj) else { return -2 };
    inner.store.get(o, &*id, buf as *mut u8, len as usize) as _
}

unsafe extern "C" fn state_put(_n: *mut zt::ZT_Node, uptr: *mut core::ffi::c_void, _t: *mut core::ffi::c_void,
    obj: zt::ZT_StateObjectType, id: *const u64, data: *const core::ffi::c_void, len: core::ffi::c_int) -> core::ffi::c_int {
    let inner = &*(uptr as *const Inner);
    // planet 回写用 PlanetPut(根下 planet 文件);identity.secret 强制 secure
    ... 同 get 的映射,PLANET→PlanetPut;secure = matches!(obj, IDENTITY_SECRET | PLANET)
    inner.store.put(o, &*id, data as *const u8, len as isize, secure) as _
}

unsafe extern "C" fn wire_send(_n: *mut zt::ZT_Node, uptr: *mut core::ffi::c_void, _t: *mut core::ffi::c_void,
    _local_socket: i64, remote: *const libc::sockaddr_storage, data: *const core::ffi::c_void, len: core::ffi::c_uint, _ttl: core::ffi::c_uint) -> core::ffi::c_int {
    let inner = &*(uptr as *const Inner);
    let Some(addr) = from_sockaddr_storage(remote) else { return -1 };
    let bytes = std::slice::from_raw_parts(data as *const u8, len as usize);
    match inner.udp.send_to(bytes, addr) { Ok(_) => 0, Err(_) => -1 }
}

unsafe extern "C" fn vnet_frame(_n: *mut zt::ZT_Node, uptr: *mut core::ffi::c_void, _t: *mut core::ffi::c_void,
    nwid: u64, _nuptr: *mut *mut core::ffi::c_void, src: u64, dst: u64, eth: core::ffi::c_uint, _vlan: core::ffi::c_uint,
    data: *const core::ffi::c_void, len: core::ffi::c_uint) {
    let inner = &*(uptr as *const Inner);
    let payload = unsafe { std::slice::from_raw_parts(data as *const u8, len as usize) };
    let _ = inner.frame_out.send(FrameOut::Vnet { nwid, src, dst, eth: eth as u16, payload: payload.to_vec() });
}

unsafe extern "C" fn vnet_config(_n: *mut zt::ZT_Node, uptr: *mut core::ffi::c_void, _t: *mut core::ffi::c_void,
    nwid: u64, _nuptr: *mut *mut core::ffi::c_void, op: zt::ZT_VirtualNetworkConfigOperation, cfg: *const zt::ZT_VirtualNetworkConfig) -> core::ffi::c_int {
    // 拷贝后经通道交驱动线程转 sink(本任务先丢弃);回调内不得调用 node 方法(死锁)
    let _ = NetConfig::from_zt(&*cfg); 0
}

unsafe extern "C" fn on_event(_n: *mut zt::ZT_Node, uptr: *mut core::ffi::c_void, _t: *mut core::ffi::c_void,
    ev: zt::ZT_Event, _meta: *const core::ffi::c_void) {
    // 映射:UP/DOWN/ONLINE/OFFLINE/IDENTITY_COLLISION;TRACE/REMOTE_TRACE 忽略
    // 经通道交驱动线程转 sink(本任务先丢弃)
}
```

(实现者注意:`...` 两处为机械补全——错误码映射与 state_put 映射,注释已给全规则;`vnet_config`/`on_event` 本任务先建好 channel 骨架,真正消费在 Task 3。)

- [ ] **Step 6: 离线验证(identity 生成与持久化,不依赖网络)**

在 `crates/pocs` 建 **占位 bin**(Task 3 会扩展):

`crates/pocs/Cargo.toml`:
```toml
[package]
name = "pocs"
version = "0.1.0"
edition = "2021"
license = "MPL-2.0"

[dependencies]
zt-core = { path = "../zt-core" }
clap = { version = "4", features = ["derive"] }
```

`crates/pocs/src/bin/poc1_control.rs`(本任务最小版):
```rust
use zt_core::{NodeHandle, NodeParams};
fn main() {
    let dir = std::path::PathBuf::from("target/poc-data/node-a");
    let h = NodeHandle::start(NodeParams {
        data_dir: dir.clone(),
        planet_file: Some("crates/pocs/fixtures/planet.Gozi25".into()),
        bind_port: 9994,
    }).expect("node start");
    println!("address = {:010x}", h.address());
    h.shutdown();
}
```

```bash
mkdir -p crates/pocs/fixtures && cp /Users/zivyuan/GoziLib/Zerotier/switch-planet/planets/planet.Gozi25 crates/pocs/fixtures/
cargo run -p pocs --bin poc1_control
# 预期:address = 10 位十六进制;target/poc-data/node-a/ 下出现 identity.public/identity.secret/planet
# planet 文件 hash 与 fixtures 副本一致
shasum -a 256 crates/pocs/fixtures/planet.Gozi25 target/poc-data/node-a/planet
ls -l target/poc-data/node-a/identity.secret   # 权限 600
cargo run -p pocs --bin poc1_control   # 二次运行:address 与首次相同(复用 identity)
```

- [ ] **Step 7: 提交**

```bash
git --no-pager add crates/zt-core crates/pocs Cargo.toml
git commit -m "feat(core): zt-core 封装层与状态存储

StateStore 按 Android stateFile 语义平移(identity 根共享、
moon/peer/netconf 按 worldId 分目录、planet 经 get 注入/put 回写,
原子写与 0600 收紧);6 必填回调 glue 以 uptr 还原 Inner;NodeHandle
构造即驱动一次后台任务完成 identity 生成。poc1 离线验证 identity
持久化与二次复用。"
```

---

### Task 3: 驱动线程、UDP 通路与 poc1 控制面

**Files:**
- Modify: `crates/zt-core/src/node.rs`(驱动线程、命令通道、join/leave/status/networks)
- Modify: `crates/pocs/src/bin/poc1_control.rs`(完整控制面验证)

**Interfaces:**
- Produces: `NodeHandle::{join(nwid: u64) -> Result<(), NodeError>, leave(nwid: u64) -> Result<(), NodeError>, networks() -> Vec<NetConfig>, online() -> bool}`;`NodeSink` 回调开始真实送达(sink 在驱动线程被调用)

- [ ] **Step 1: 驱动线程(poll:UDP fd + wakeup pipe;设备帧/命令/组播订阅)**

结构(完整实现,依赖 `libc` pipe/poll——不用额外 nix 依赖):

```rust
// Inner 增加:
pub(crate) cmd_tx: Sender<Cmd>,
pub(crate) wake_tx: /* pipe 写端(包装为 File) */,
enum Cmd { Join(u64), Leave(u64), Stop }

// start() 尾部:
//   let (cmd_tx, cmd_rx) = channel(); let (frame_out, frame_rx) = channel()(Task 2 已建)
//   let (wake_rd, wake_wr) = pipe2(O_CLOEXEC) via libc
//   spawn driver_thread(node, inner, cmd_rx, frame_rx, wake_rd, sink(移入线程))
//   驱动线程持有 sink;NodeHandle 侧只留 node/inner/命令通道

fn driver_thread(node: *mut zt::ZT_Node, inner: *mut Inner, mut sink: Box<dyn NodeSink>,
                 cmd_rx: Receiver<Cmd>, frame_rx: Receiver<FrameOut>,
                 wake_rd: RawFd, mut deadline: i64) {
    let udp_fd = unsafe { &*inner }.udp.as_raw_fd();
    let mut last_mcast_ms: i64 = 0;
    let mut current_networks: Vec<u64> = vec![];   // 已 join(来自 config 回调,经另一通道或 networks() 快照)
    let mut polls = [pollfd { fd: udp_fd, events: POLLIN, revents: 0 },
                     pollfd { fd: wake_rd, events: POLLIN, revents: 0 }];
    loop {
        let now = now_ms();
        if deadline <= now {
            let mut dl: i64 = 0;
            let rc = unsafe { zt::ZT_Node_processBackgroundTasks(node, std::ptr::null_mut(), now as _, &mut dl) };
            deadline = dl;
            if rc as i32 >= 100 { sink.on_event(NodeEvent::Fatal(format!("core rc={rc}"))); break; }
        }
        // 命令(本线程执行,保证 process* 串行)
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                Cmd::Join(n) => { let _ = unsafe { zt::ZT_Node_join(node, n, std::ptr::null_mut(), std::ptr::null_mut()) }; },
                Cmd::Leave(n) => { let _ = unsafe { zt::ZT_Node_leave(node, n, std::ptr::null_mut(), std::ptr::null_mut()) }; },
                Cmd::Stop => return,   // shutdown 已先置 running=false
            }
        }
        // 核心生成的虚拟网络帧(vnet_frame 回调经 frame_out 通道到此处)→ 交 sink
        // (POC1 无设备,sink 只打印/计数;Task 4 的 sink 写设备)。**不得**回喂
        // processVirtualNetworkFrame——那是设备收帧方向(FrameIn 通道,Task 4 加)
        while let Ok(f) = frame_rx.try_recv() {
            let FrameOut::Vnet { nwid, src, dst, eth, payload } = f;
            sink.on_frame(nwid, src, dst, eth, &payload);
        }
        // 组播订阅:每 5000ms 对本机每个 IPv4 订阅 (ffffffffffff, ADI=主机序)
        if now - last_mcast_ms >= 5000 {
            last_mcast_ms = now;
            for nwid in &current_networks {
                for ip in sink.local_ipv4s() {
                    let _ = unsafe { zt::ZT_Node_multicastSubscribe(node, std::ptr::null_mut(), *nwid, 0xffff_ffff_ffff, u32::from(ip) as _) };
                }
            }
        }
        // poll
        let timeout = (deadline - now).clamp(1, 500) as i32;
        let n = unsafe { libc::poll(polls.as_mut_ptr(), 2, timeout) };
        if n > 0 && polls[0].revents & POLLIN != 0 {
            let mut buf = [0u8; 10100 + 224];   // ZT_MAX_PHYSPAYLOAD + ZT_MAX_HEADROOM
            while let Ok((len, from)) = unsafe { &*inner }.udp.recv_from(&mut buf) {
                let mut dl: i64 = 0;
                let sa = to_sockaddr_storage(&from, None);
                let _ = unsafe { zt::ZT_Node_processWirePacket(node, std::ptr::null_mut(), now_ms() as _, udp_fd as _, &sa, buf.as_ptr() as *const _, len as _, &mut dl) };
                if dl > 0 && dl < deadline { deadline = dl; }
            }
        }
        if n > 0 && polls[1].revents & POLLIN != 0 {
            let mut b = [0u8; 64]; let _ = unsafe { libc::read(wake_rd, b.as_mut_ptr() as *mut _, 64) };
        }
        if !unsafe { &*inner }.running.load(Ordering::SeqCst) { return; }
    }
}
```

(实现者注意:`current_networks` 由 vnet_config 回调经通道(新增 `cfg_rx`,与 FrameOut 同一 enum 扩展 `Config{op, cfg: NetConfig}`)更新,并在同处调 `sink.on_net_config`;`on_event` 同理经通道;这些 channel 的发送端在 Task 2 的回调 glue 里已预留,此处补消费。poll 常量经 `libc::POLLIN`。)`vnet_frame` 在 POC1 下无设备,sink.on_frame 仅供 Task 4。

`NodeHandle::shutdown` 顺序(强制):`running=false` → 写 wake pipe 唤醒 → `cmd_tx.send(Cmd::Stop)` → join 驱动线程 → `ZT_Node_delete` → drop Inner。

- [ ] **Step 2: join/leave/status 封装**

```rust
impl NodeHandle {
    pub fn join(&self, nwid: u64) -> Result<(), NodeError> { self.inner().cmd_tx.send(Cmd::Join(nwid)).map_err(|_| NodeError::DataStoreFailed("driver 线程已退出".into())) }
    pub fn leave(&self, nwid: u64) -> Result<(), NodeError> { ...同上 Leave... }
    pub fn networks(&self) -> Vec<NetConfig> {
        unsafe {
            let l = zt::ZT_Node_networks(self.node);
            let mut out = vec![];
            if !l.is_null() {
                for i in 0..(*l).networkCount {
                    out.push(NetConfig::from_zt(&*(*l).networks.add(i)));
                }
                zt::ZT_Node_freeQueryResult(self.node, l as *mut _);
            }
            out
        }
    }
    pub fn online(&self) -> bool { unsafe { let mut s: zt::ZT_NodeStatus = std::mem::zeroed(); zt::ZT_Node_status(self.node, &mut s); s.online != 0 } }
}
```

- [ ] **Step 3: poc1_control 完整版**

```rust
// 参数:--data-dir <dir>(默认 target/poc-data/node-a) --port <u16 默认 9994>
//       --planet <path 默认 crates/pocs/fixtures/planet.Gozi25>
//       [--join <nwid 十六进制>] [--duration <秒 默认 300>]
// 每 2s 打印:address/online/各网络 status/netconfRevision/assigned/routes
// sink:打印 on_event 与 on_net_config 摘要
// 结束:打印最终 networks 全量 JSON(serde_json 可省,手写 Debug 输出即可),shutdown
```

(注:poc1 需定义一个最小 `PrintSink impl NodeSink`——on_event/on_net_config 打印、on_frame 计数、local_ipv4s 返回空;Task 4 的 poc2 换成 DevicePlane 驱动的 sink。帧方向严格区分:**FrameOut = 核心产出帧 → sink 写设备;FrameIn = 设备收到帧 → frame_in 通道 → 驱动线程调 processVirtualNetworkFrame**。两条通道,方向不可混。)

- [ ] **Step 4: 验证**

```bash
cargo run -p pocs --bin poc1_control -- --join 5cf54f181554090f --duration 60
# 段 a(实现者自验):事件序列 UP → ONLINE;网络 status = REQUESTING_CONFIGURATION
#   (控制器未授权时的稳态);Ctrl-C 或 60s 后干净退出,无 panic
# 段 b(需用户):用户在控制器授权新节点后,复跑 --duration 300:
#   status = OK,打印 assigned addresses(如 10.x.x.x)与 managed routes
```

- [ ] **Step 5: 提交**

```bash
git --no-pager add crates/zt-core crates/pocs
git commit -m "feat(core): 驱动线程与 UDP 通路,POC1 控制面打通

单驱动线程串行执行全部 process*(poll UDP+唤醒管道,deadline 到点
驱动后台任务);join/leave/networks/online 封装;poc1 对 Gozi25
join 5cf54f181554090f 走到 OK 并回显分配地址与路由。"
```

---

### Task 4: 设备层(feth L2)与配置应用,poc2 ping 通

**Files:**
- Create: `crates/zt-core/src/device.rs`、`crates/zt-core/src/routes.rs`
- Modify: `crates/zt-core/src/lib.rs`(pub mod)、`crates/pocs/src/bin/poc2_device.rs`(新建)
- Modify: `crates/zt-core/Cargo.toml`(加 tun-rs、route_manager)

**Interfaces:**
- Consumes: Task 3 的 `on_net_config/on_frame/local_ipv4s`、`FrameOut::Vnet`
- Produces: `pub struct DevicePlane`(pocs 用);`zt-core` 依赖 `tun-rs = { version = "2", default-features = false }`(同步模式,不需要 address-management?——保留 default,用其 add_address_v4)、`route_manager = "0.2"`

- [ ] **Step 1: DevicePlane(设备 + 配置应用 + 帧环回)**

```rust
//! 每网络一张 L2 设备(feth 对,内核处理 ARP);managed IP/route 应用器。
use crate::config::*;
use route_manager::{Route, RouteManager};
use std::net::{IpAddr, Ipv4Addr};
use tun_rs::{Device as _, SyncDevice};

pub struct DevicePlane {
    dev: Option<SyncDevice>,          // 单网络 POC;daemon 期改为 per-nwid 表
    applied_addrs: Vec<IpNet>,
    applied_routes: Vec<ManagedRoute>,
    rm: RouteManager,
    /// 设备线程句柄与停止标志
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    dev_thread: Option<std::thread::JoinHandle<()>>,
}

impl DevicePlane {
    /// on_net_config(Up/ConfigUpdate) 调用;Down/Destroy 拆除
    pub fn apply(&mut self, cfg: &NetConfig, frame_tx: &dyn Fn(FrameIn)) -> std::io::Result<()> {
        if self.dev.is_none() {
            let dev = tun_rs::DeviceBuilder::new()
                .layer(tun_rs::Layer::L2)
                .mtu(cfg.mtu.max(1280))
                .build_sync()?;                       // sudo 要求点
            let name = dev.name()?;
            println!("[device] {} mtu={}", name, cfg.mtu);
            self.spawn_reader(dev.try_clone_fd_or_handle(), frame_tx);  // 见下注
            self.dev = Some(dev);
        }
        // 地址差分应用
        for a in &cfg.assigned { if !self.applied_addrs.contains(a) { self.add_addr(a)?; } }
        for a in &self.applied_addrs.clone() { if !cfg.assigned.contains(a) { self.del_addr(a)?; } }
        // 路由差分:via 为 None 的 on-link 路由用 if_index;有 via 加 gateway
        // 0.0.0.0/0 与 ::/0 目标一律跳过(不劫持默认路由)
        ...
        self.applied_addrs = cfg.assigned.clone();
        Ok(())
    }
    fn add_addr(&mut self, n: &IpNet) -> std::io::Result<()> { /* v4: add_address_v4;v6: add_address_v6;同步维护 applied */ }
    pub fn teardown(&mut self) { /* stop 线程 → drop dev(utun/feth 随 fd 销毁)→ 删除已加路由 */ }
    fn spawn_reader(&mut self, fd_or_dev: ?, frame_tx: &dyn Fn(FrameIn)) { /* 阻塞 recv Ethernet 帧 → 拆 14B 头(dst,src,ethType) → frame_tx(FrameIn{nwid,src,dst,eth,payload}) */ }
}

pub struct FrameIn { pub nwid: u64, pub src: u64, pub dst: u64, pub eth: u16, pub payload: Vec<u8> }
```

(实现者注意两处需现场核实的 API 事实,计划已给核实路径:
① `SyncDevice` 线程安全克隆/取 fd——查 tun-rs 源码 `src/platform/unix/tun.rs` 的 `try_clone`(Linux only)与 macOS 等价物;若无,设备线程直接**持有** dev,主侧只留 name/if_index 做地址路由操作(地址操作经 `route_manager`+自建 ioctl 或 `netsh` 类——macOS 用 `libc::if_nametoindex` + `SIOCAIFADDR` 需要 fd?不需要,ioctl 走任意 UDP socket + ifreq,按 if_name 指定设备)。**推荐做法:设备线程持有 dev 收帧;地址/路由全部经 route_manager + 自写 ifreq ioctl helper(set_ip_for_if),避免对 tun-rs 内部 API 的依赖**。)
② feth 对端帧是否带 14B 以太头——写一个 5 行 echo 探针确认,再定帧头解析。

- [ ] **Step 2: ioctl helper(ifreq 设地址,macOS)**

```rust
/// 对指定网络接口设 IPv4 地址(SIOCAIFADDR)/删(SIOCDIFADDR)、IPv6(SIOCAIFADDR_IN6)。
/// 走任意 datagram socket + ifreq,不依赖 tun-rs 设备对象。
pub fn set_ipv4(if_name: &str, addr: Ipv4Addr, prefix: u8, remove: bool) -> std::io::Result<()> {
    // ifaliasreq { ifra_name[16], ifra_addr(sockaddr_in), ifra_broadaddr(掩码), ifra_mask }
    // 掩码 sockaddr_in: sin_addr = prefix 掩码;remove 时 addr=0 走 SIOCDIFADDR
    ...
}
pub fn set_ipv6(if_name: &str, addr: Ipv6Addr, prefix: u8, remove: bool) -> std::io::Result<()> { /* in6_ifaliasreq,IN6_IFF_NODAD */ }
```

(实现者注意:ifreq 家族结构体在 `libc` 中有 `ifaliasreq`/`in6_ifaliasreq`(macOS);对照 tun-rs `src/platform/macos/` 源码抄参数细节,那是最权威的本地参考。)

- [ ] **Step 3: poc2_device bin**

```rust
// 参数同 poc1,增加 --target <IPv4>(可选,网内既有成员,bin 内起 /sbin/ping 每 5s 一次)
// 流程:start node → join → sink.on_net_config(Up/ConfigUpdate)时 DevicePlane::apply
//   sink.on_frame → dev 发帧(经设备线程);sink.local_ipv4s 返回 applied v4 地址
//   FrameIn 由设备线程经 zt-core 通道回喂驱动线程(复用 FrameOut 通道反向)
// --duration 默认 600;结束时 teardown + shutdown
```

帧通路接线(zt-core 侧):`on_frame`(sink 方向)= DevicePlane 向设备写帧;设备线程产出的 `FrameIn` 走**新增的 frame_in 通道**进驱动线程调 `processVirtualNetworkFrame`(与 frame_out 两条独立通道,方向不可混)。

- [ ] **Step 4: 验证(sudo 由用户执行)**

```bash
cargo build -p pocs --bin poc2_device
# 用户执行(授权点②):
sudo ./target/debug/poc2_device --join 5cf54f181554090f --duration 600
# 预期:创建 feth 设备并打印名称;配置 assigned IPv4;路由表出现 managed 网段(ifconfig/ netstat -rn 验证,无默认路由劫持)
# 从网内另一设备 ping <assigned-ip> → 通;或本机 sudo ./poc2 … --target <成员IP> 自 ping
# 退出后:feth 消失(ifconfig | grep feth),新增路由清除
```

- [ ] **Step 5: 提交**

```bash
git --no-pager add crates/zt-core crates/pocs
git commit -m "feat(core): feth L2 设备层与 managed 配置应用,POC2 ping 通

DevicePlane 每网络一张 feth(内核 ARP),ifreq ioctl 设地址、
route_manager 差分应用 managed 路由(跳过默认路由),设备线程
阻塞收帧拆以太头回喂核心;poc2 实测网内互 ping。"
```

---

### Task 5: poc3 双 Node 并发与隔离验证

**Files:**
- Create: `crates/pocs/src/bin/poc3_multi.rs`
- Modify: 无 zt-core 代码(若发现串扰问题,如实记录为 BLOCKED/DONE_WITH_CONCERNS,不改架构)

- [ ] **Step 1: poc3_multi bin**

```rust
// 两 NodeHandle:node-a(port 9994, data target/poc-data/node-a)
//               node-b(port 9995, data target/poc-data/node-b)
// 同一 planet 文件,各自独立 identity(Task 2 已天然隔离)
// 双双 join 5cf54f181554090f(需授权两个新设备)
// 每 2s 打印双节点 address/online/status/assigned
// 互 ping:node-a 侧起 ping node-b 的 assigned-ip(走各自 DevicePlane)
// 验证项逐条断言并打印 PASS/FAIL:
//   1. 两 identity.secret 内容不同且都在各自目录
//   2. 两 dataStore 目录无交叉文件(node-b 的 peers.d 不出现在 node-a)
//   3. 两 UDP 端口各自独立 bind 成功
//   4. 运行中 kill 场景不做(POC 无信号处理);改为:node-b shutdown 后重建
//      (shutdown → 重新 start 复用 dataStore),node-a 网络不掉线
//   5. a↔b 互 ping 通(走 feth)
// 内存基线:/usr/bin/time -l 由外部命令采集,bin 不自测
```

- [ ] **Step 2: 验证(sudo + 控制器授权两个设备,用户配合)**

```bash
cargo build -p pocs --bin poc3_multi
sudo /usr/bin/time -l ./target/debug/poc3_multi --duration 600 2>&1 | tee /tmp/poc3.log
# 预期:5 项全 PASS;日志含 "maximum resident set size"(记录为基线)
# 控制器授权两个新节点后 status 双 OK
```

- [ ] **Step 3: 结果记录与提交**

POC 结论写回 `docs/superpowers/specs/2026-10-28-daemon-poc.md` 末尾追加"POC 结果记录"小节(三项验收逐条 ✓/✗ + 实测数字 + 内存基线)。

```bash
git --no-pager add crates/pocs docs/superpowers/specs/2026-10-28-daemon-poc.md
git commit -m "feat(pocs): 双 Node 并发隔离验证,POC3 通过

同进程两 Node(同 planet、独立 identity/端口/dataStore)并发
在线互 ping,五项隔离断言全过并记录内存基线;spec 追加 POC
结果记录,三项验收结论留档。"
```

---

## 执行注意(给 controller)

- Task 3 段 b、Task 4 全部、Task 5 全部依赖用户配合(控制器授权 / sudo),实现者完成可自验部分后,由 controller 把精确命令与预期转给用户,拿到输出再判定
- Task 4 两处"现场核实 API 事实"(tun-rs 设备线程化方式、feth 帧头)在实现中核实,若与计划假设不符,按注记的推荐做法调整并写入报告,不必回报等待
- 若 POC 3 发现互扰:停,报告现象,不擅自改架构(形态 B 决策权在用户)
