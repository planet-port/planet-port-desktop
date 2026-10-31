# Windows 支持评估 POC 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在实体 Win10 机器上以三道 Gate（构建链路 → wintun 设备层 → 组网互通）验证 Windows 支持可行性，产出实测记录与 wintun 路线结论。

**Architecture:** 新增 `poc4_windows`（仅依赖 zt-sys FFI + tun-rs + route_manager，**不碰 zt-core**），自写 Windows 版 node 驱动（UDP 读线程 + channel 唤醒替代 Unix pipe/poll）与 IPv4 ARP 垫层。开发在 mac、构建与运行经 ssh 驱动 Win10 实机。规格见 `docs/superpowers/specs/2026-10-31-windows-poc-design.md`。

**Tech Stack:** Rust（MSVC 工具链）、zt-sys（cc + bindgen）、tun-rs 2.x（wintun L3，address-management）、route_manager 0.2、wintun.dll 0.14.1。

## Global Constraints

- Windows 代码只进新文件（`crates/pocs/src/poc4/**`、`crates/pocs/src/bin/poc4_windows.rs`）与 pocs 的 `[target.'cfg(windows)'.dependencies]`；**禁止修改 zt-core / daemon / cli / api 与现有脚本**
- 所有 Windows 模块以 `#[cfg(windows)]` 隔离；mac 上 `cargo build --workspace` 必须保持成功（Task 8 验证）
- 不添加测试用例（用户全局规则）；验证方式 = Win10 实机运行 + 观察输出
- 任一 Gate 失败即停，定位归因，不打补丁绕过
- 提交信息：Conventional Commits + 中文 + Body，禁 Co-authored-by
- ssh 目标机别名统一写作 `win10`（实际 host/user 在 Task 1 由用户提供后回填本文件的"环境信息"节）
- POC 范围 IPv4；IPv6/NDP 不做
- planet 用 `crates/pocs/fixtures/planet.Gozi25`，测试网络 `5cf54f181554090f`（与 poc3 一致）

## 环境信息（Task 1 执行后回填）

- Win10 版本/build：（待回填，`winver` 输出）
- 架构：（待回填，预期 x64）
- ssh 连接方式：（待回填，如 `ssh administrator@192.168.x.x`）
- 代码落盘目录：`C:\planet-port`（固定）

---

### Task 1: Win10 实机盘点与 ssh 打通

**Files:** 无代码文件；产出 = 回填本计划"环境信息"节 + compatibility.md 草稿条目。

**Interfaces:**
- Produces: ssh 可用通道 `win10`（后续所有任务的执行前提）；环境信息回填。

- [ ] **Step 1: 向用户要连接参数**

需要用户提供：Win10 实机的局域网 IP、一个管理员账户用户名与密码。拿到后在 mac 的 `~/.ssh/config` 加别名（该文件在工作区外，请用户自己加或明确授权修改）：

```
Host win10
    HostName <IP>
    User <user>
```

- [ ] **Step 2: 查系统版本与架构**

```
ssh win10 "cmd /c ver & systeminfo | findstr /B /C:\"OS Name\" /C:\"OS Version\" /C:\"System Type\""
```

Expected: Microsofthttps Windows 10 ... / x64-based PC。把输出回填本计划"环境信息"节。

- [ ] **Step 3: 开启内置 OpenSSH Server（若未开）**

```
ssh win10 "powershell -Command \"Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0; Set-Service sshd -StartupType Automatic; Start-Service sshd\""
```

Expected: OpenSSH.Server 状态 Installed / Running（已装则提示已存在，正常）。

- [ ] **Step 4: 确认管理员身份与免密可行**

管理员身份验证方式二选一：用户已在 win10 上把 mac 的公钥加入 `administrators_authorized_keys`，或 ssh 用密码（mac 侧用 `sshpass` 或每次交互输密码；计划内命令统一走 `ssh win10`，执行时按实际填）。验证：

```
ssh win10 "net session >nul 2>&1 && echo ADMIN || echo NOT-ADMIN"
```

Expected: `ADMIN`（POC 全程需要管理员：wintun 建适配器、改路由）。

- [ ] **Step 5: 建工作目录**

```
ssh win10 "if not exist C:\\planet-port mkdir C:\\planet-port && echo OK"
```

Expected: `OK`。

---

### Task 2: Win10 工具链安装（VS Build Tools + LLVM + Rust）

**Files:** 无代码文件；产出 = Win10 上可用的 cl.exe / libclang / cargo。

**Interfaces:**
- Produces: Task 3 的构建前提；环境变量 `LIBCLANG_PATH=C:\Program Files\LLVM\bin`（后续命令直接引用）。

- [ ] **Step 1: 安装 VS Build Tools（C++ 工作负载）**

winget 在老版 Win10 可能缺失，统一用官方引导脚本（直链微软域名，网络可达）：

```
ssh win10 "powershell -Command \"Invoke-WebRequest -Uri https://aka.ms/vs/17/release/vs_BuildTools.exe -OutFile C:\\vs_BuildTools.exe\""
ssh win10 "C:\\vs_BuildTools.exe --quiet --wait --norestart --nocache --installPath C:\\BuildTools --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

Expected: 第二条静默执行数分钟后退出码 0。验证：`ssh win10 "dir C:\\BuildTools\\VC\\Tools\\MSVC"` 列出版本目录。若早期 Win10 装不上 2022 版（返回失败），改用 vs_BuildTools 2019 链接 `https://aka.ms/vs/16/release/vs_BuildTools.exe` 重试（设计文档风险表已预案）。

- [ ] **Step 2: 安装 LLVM（bindgen 依赖 libclang）**

```
ssh win10 "powershell -Command \"Invoke-WebRequest -Uri https://github.com/llvm/llvm-project/releases/download/llvmorg-18.1.8/LLVM-18.1.8-win64.exe -OutFile C:\\llvm.exe\""
```

GitHub 直链受限时改用镜像：`https://mirrors.tuna.tsinghua.edu.cn/github-release/llvm/llvm-project/` 下同名资产，或用户手动下载后放到 `C:\llvm.exe`。安装：

```
ssh win10 "C:\\llvm.exe /S"
```

Expected: `C:\Program Files\LLVM\bin\libclang.dll` 存在。

- [ ] **Step 3: 安装 Rust（MSVC 工具链，走 rsproxy 镜像）**

PowerShell 一条链（注意 rsproxy 环境变量要在安装前设置）：

```
ssh win10 "powershell -Command \"$env:RUSTUP_DIST_SERVER='https://rsproxy.cn'; $env:RUSTUP_UPDATE_ROOT='https://rsproxy.cn/rustup'; Invoke-WebRequest -Uri https://rsproxy.cn/rustup-init.exe -OutFile C:\\rustup-init.exe; C:\\rustup-init.exe -y --default-toolchain stable-x86_64-pc-windows-msvc --default-host x86_64-pc-windows-msvc\""
```

再配置 crates 镜像（写 `%USERPROFILE%\.cargo\config.toml`）：

```toml
[source.crates-io]
replace-with = 'rsproxy-sparse'
[source.rsproxy-sparse]
registry = "sparse+https://rsproxy.cn/index/"
```

Expected: `ssh win10 "cargo --version && rustc --version"` 正常输出。

- [ ] **Step 4: 验证 MSVC 工具链可用（vcvars 环境下编译 hello）**

后续所有 cargo 命令都必须在 vcvars 环境下跑。封装一条固定命令模板（下文统称 **BUILD_CMD**）：

```
ssh win10 "cmd /c \"call C:\\BuildTools\\VC\\Auxiliary\\Build\\vcvars64.bat && <命令>\""
```

验证：

```
ssh win10 "cmd /c \"call C:\\BuildTools\\VC\\Auxiliary\\Build\\vcvars64.bat && where cl && cl /nologo 2>&1 | findstr /C:\"Compiler\"\""
```

Expected: cl.exe 路径 + 版本行。同时设 `set LIBCLANG_PATH=C:\Program Files\LLVM\bin`（并入 BUILD_CMD 模板）。

---

### Task 3: 代码同步 + Gate 0（zt-sys 在 Win10 编译通过）

**Files:**
- 无仓库代码改动（只构建）。

**Interfaces:**
- Consumes: Task 1 的 `win10` 通道、Task 2 的 BUILD_CMD。
- Produces: **Gate 0 结论**；Win10 上 `C:\planet-port` 的完整 workspace（含 mac 侧已拉取的 `Core/` 源码）。

- [ ] **Step 1: 打包同步 workspace 到 Win10**

`Core/`（ZeroTier 源码）由 `scripts/bootstrap.sh` 拉取、不入库，不能靠 git 同步；直接整树打包（Win10 1803+ 自带 tar）。在 mac 项目根目录执行：

```bash
tar czf /tmp/pp.tar.gz --exclude target --exclude dist --exclude .git --exclude claude-session .
scp /tmp/pp.tar.gz win10:C:/planet-port/
ssh win10 "cmd /c \"cd /d C:\\planet-port && tar xzf pp.tar.gz && del pp.tar.gz\""
```

Expected: `C:\planet-port` 下出现 Cargo.toml、Core、crates 等。后续每次代码更新重复本步（封装成 mac 侧脚本 `scripts/sync-win10.sh`，见 Step 4）。

- [ ] **Step 2: Gate 0 构建**

```
ssh win10 "cmd /c \"cd /d C:\\planet-port && call C:\\BuildTools\\VC\\Auxiliary\\Build\\vcvars64.bat && set LIBCLANG_PATH=C:\\Program Files\\LLVM\\bin && cargo build -p planet-port-zt-sys\""
```

Expected（通过）: `Finished \`debug\` profile`。
失败排查顺序：① bindgen 报 libclang 找不到 → 检查 LIBCLANG_PATH；② cc 报 cl.exe 不存在 → vcvars64 未生效；③ C++ 编译错误 → 记录具体文件与错误码，**停下归因**（可能是 MSVC 对 ZeroTier 核心子集的兼容问题，属 Gate 0 要暴露的核心风险）。

- [ ] **Step 3: 检查 Windows 侧 bindgen 产物差异（为 Task 6 铺路）**

```
ssh win10 "cmd /c \"findstr /B /C:\"pub const AF_INET\" /C:\"pub struct sockaddr_in \" /C:\"pub struct in6_addr\" C:\\planet-port\\target\\debug\\build\\zt-sys-*\\out\\bindings.rs\""
```

记录三项实际形态：`AF_INET` 的类型（u32/i32）、`in6_addr` 的 union 字段名（Windows 预期 `u.Byte`）、`sockaddr_in.sin_family` 类型（预期 ADDRESS_FAMILY=u16）。Task 6 的 `poc4/sa.rs` 按实测结果核对（代码已按预期形态写好，出入就地修正）。

- [ ] **Step 4: 封装同步脚本（mac 侧）**

创建 `scripts/sync-win10.sh`（Step 1 的三条命令，参数化 tar 包路径；`#!/usr/bin/env bash`、`set -euo pipefail`，与 scripts/ 现有脚本风格一致）。

- [ ] **Step 5: 提交**

```bash
git add scripts/sync-win10.sh
git commit -m "chore(scripts): 新增 Win10 评估机代码同步脚本

Windows POC 评估用:tar 打包工作树(含不入库的 Core/ 源码)经
ssh 推送到 Win10 实机,规避在 Windows 上跑 bash bootstrap。"
```

---

### Task 4: poc4 骨架 + Gate 1a（wintun 设备创建/销毁）

**Files:**
- Modify: `crates/pocs/Cargo.toml`
- Create: `crates/pocs/src/bin/poc4_windows.rs`
- Create: `crates/pocs/src/poc4/mod.rs`、`crates/pocs/src/poc4/device_win.rs`

**Interfaces:**
- Consumes: Task 3 的 Win10 构建环境。
- Produces: `poc4::device_win::WinDevice`（`new(name: &str, mtu: u16) -> io::Result<Self>`、`recv(&self, buf: &mut [u8]) -> io::Result<usize>`、`send(&self, pkt: &[u8]) -> io::Result<usize>`、`if_index()`、`name()`、`add_address_v4(v4, prefix)`、`remove_address(ip)`），Task 5/7 复用。

- [ ] **Step 1: pocs 增加仅 Windows 的依赖**

`crates/pocs/Cargo.toml` 末尾追加：

```toml
# Windows 支持评估 POC(poc4):wintun L3 设备 + managed 路由 + zt-sys 直连 FFI
[target.'cfg(windows)'.dependencies]
zt-sys = { path = "../zt-sys" }
tun-rs = { version = "2", default-features = false, features = ["address-management"] }
route_manager = "0.2"
```

- [ ] **Step 2: 写 bin 骨架**

`crates/pocs/src/bin/poc4_windows.rs`：

```rust
//! POC 4:Windows 支持可行性(wintun L3 + ZeroTier 核心,最低 Win10)。
//! 三道 Gate:G0 构建链路(已由 zt-sys 构建通过)/ G1 设备层 / G2 组网互通。
//! 非 Windows 平台仅占位,保证 workspace 全平台可编译。
#[cfg(windows)]
#[path = "../poc4/mod.rs"]
mod poc4;

#[cfg(windows)]
fn main() {
    poc4::main_win();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("poc4_windows 仅在 Windows 上运行(Windows 支持评估 POC)");
}
```

- [ ] **Step 3: 写设备模块（wintun L3）**

`crates/pocs/src/poc4/mod.rs`：

```rust
//! poc4 模块根:仅 Windows 编译。各子模块职责见文件头注释。
mod device_win;
pub(crate) mod arp;
pub(crate) mod driver;
pub(crate) mod netcfg;
pub(crate) mod sa;
pub(crate) mod store;

/// 入口(bin/poc4_windows.rs 调用)。
pub fn main_win() {
    let code = crate_main();
    std::process::exit(code);
}

fn crate_main() -> i32 {
    // Task 4 先落设备层自验(Gate 1),Task 6/7 在此扩展控制面与数据面入口。
    device_win::gate1_cli()
}
```

`crates/pocs/src/poc4/device_win.rs`：

```rust
//! wintun(L3)设备封装:tun-rs 同步设备 + managed 地址(复用其
//! address-management/netconfig-rs)。Gate 1 自验入口也在此。
//!
//! 与 zt-core 的 device.rs 的关键差异:wintun 是 L3 设备——
//! - 无以太网头:send/recv 都是裸 IP 包(ARP 见 arp.rs);
//! - 适配器随设备对象存活,drop 即销毁,无 feth pair 概念。
use std::io;

/// 一个 wintun 适配器。命名固定带 planet-port 前缀,便于实测时 netsh 查找。
pub struct WinDevice {
    dev: tun_rs::sync::SyncDevice,
    name: String,
    if_index: u32,
}

impl WinDevice {
    /// 创建(已存在同名适配器会复用 wintun 的 ring)。需要管理员权限。
    pub fn new(name: &str, mtu: u16) -> io::Result<Self> {
        let dev = tun_rs::DeviceBuilder::new()
            .layer(tun_rs::Layer::L3)
            .name(name)
            .mtu(mtu)
            .enable(true)
            .build_sync()?;
        let dn = dev.name()?;
        let if_index = dev
            .if_index()
            .map_err(|e| io::Error::other(format!("获取 {dn} if_index 失败: {e}")))?;
        Ok(Self { dev, name: dn, if_index })
    }

    pub fn name(&self) -> &str { &self.name }
    pub fn if_index(&self) -> u32 { self.if_index }
    /// 设备引用(读线程/写帧共用;调用方自行 Arc)
    pub fn device(&self) -> &tun_rs::sync::SyncDevice { &self.dev }
    pub fn into_inner(self) -> tun_rs::sync::SyncDevice { self.dev }

    pub fn add_address_v4(&self, addr: std::net::Ipv4Addr, prefix: u8) -> io::Result<()> {
        self.dev.add_address_v4(addr, prefix)
    }
    pub fn remove_address(&self, addr: std::net::IpAddr) -> io::Result<()> {
        self.dev.remove_address(addr)
    }

    /// L3:recv 返回裸 IP 包(无以太网头)
    pub fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        self.dev.recv(buf)
    }
    /// L3:send 写裸 IP 包(写以太网帧无意义,驱动只认 IP)
    pub fn send(&self, pkt: &[u8]) -> io::Result<usize> {
        self.dev.send(pkt)
    }
}

/// Gate 1 自验 CLI:`poc4_windows --gate1-create|--gate1-ip a.b.c.d/nn`
/// 设备创建/地址应用以实际系统状态为准,不内置断言——观察即结论。
pub fn gate1_cli() -> i32 {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--gate1-create") => {
            let d = WinDevice::new("planet-port", 2800);
            match d {
                Ok(d) => {
                    println!("[gate1] PASS 适配器已建立: name={} if_index={}", d.name(), d.if_index());
                    println!("[gate1] 按任意键销毁并退出(检查 Get-NetAdapter 后再按)");
                    let _ = std::io::stdin().read_line(&mut String::new());
                    0
                }
                Err(e) => {
                    eprintln!("[gate1] FAIL 设备创建失败: {e}");
                    1
                }
            }
        }
        Some(a) if a == "--gate1-ip" => {
            let ip_arg = args.get(2).expect("用法: --gate1-ip a.b.c.d/nn").clone();
            let (ip, prefix) = ip_arg.split_once('/').expect("前缀用 / 分隔,如 10.121.16.50/24");
            let d = match WinDevice::new("planet-port", 2800) {
                Ok(d) => d,
                Err(e) => { eprintln!("[gate1] FAIL {e}"); return 1; }
            };
            match d.add_address_v4(ip.parse().expect("非法 IPv4"), prefix.parse().expect("非法前缀")) {
                Ok(()) => {
                    println!("[gate1] PASS 地址已应用: {}/{} on {}", ip, prefix, d.name());
                    println!("[gate1] 请另开终端验证: ipconfig / route print;回车后销毁退出");
                    let _ = std::io::stdin().read_line(&mut String::new());
                    0
                }
                Err(e) => { eprintln!("[gate1] FAIL 地址应用失败: {e}"); 1 }
            }
        }
        _ => {
            eprintln!("Gate 0 已随 zt-sys 构建通过;当前版本先提供 --gate1-create / --gate1-ip");
            2
        }
    }
}
```

- [ ] **Step 4: 空 arp/sa/store/driver/netcfg 占位模块**

POC 分任务推进，为让本任务可编译，其余模块先放最小占位（后续任务替换）。`crates/pocs/src/poc4/sa.rs`：

```rust
//! sockaddr_storage <-> SocketAddr(winsock 布局,Task 6 实装)。
pub fn _placeholder() {}
```

`arp.rs`、`driver.rs`、`netcfg.rs`、`store.rs` 同样各放一行 `pub fn _placeholder() {}`（文件头注释写明"Task N 实装"）。

- [ ] **Step 5: mac 侧编译验证（零影响检查点）**

```bash
cargo build --workspace
```

Expected: 成功（poc4_windows 在 mac 上只编译占位 main）。失败则修到过。

- [ ] **Step 6: 同步到 Win10 并跑 Gate 1a**

先取 wintun.dll（tun-rs 经 wintun-bindings 加载 exe 同目录的 wintun.dll）：

```
ssh win10 "powershell -Command \"Invoke-WebRequest -Uri https://download.wintun.net/wintun-0.14.1.zip -OutFile C:\\wintun.zip; Expand-Archive C:\\wintun.zip -DestinationPath C:\\wintun\""
```

（wintun.net 非 GitHub 域，直连预期可达；失败则 mac 下载后 scp 过去，解压取 `wintun\wintun\bin\amd64\wintun.dll`。）

```bash
./scripts/sync-win10.sh
ssh win10 "cmd /c \"cd /d C:\\planet-port && call C:\\BuildTools\\VC\\Auxiliary\\Build\\vcvars64.bat && set LIBCLANG_PATH=C:\\Program Files\\LLVM\\bin && cargo build -p pocs --bin poc4_windows\""
ssh win10 "cmd /c \"copy /Y C:\\wintun\\wintun\\bin\\amd64\\wintun.dll C:\\planet-port\\target\\debug\\ && C:\\planet-port\\target\\debug\\poc4_windows.exe --gate1-create\""
```

Expected: `PASS 适配器已建立`。同时另开 `ssh win10 "powershell -Command \"Get-NetAdapter -Name planet-port\""` 能看到适配器。
失败即 **Gate 1 未过，停下归因**（常见：wintun.dll 不在 exe 目录 / 非管理员 ssh / 驱动策略拦截——记录报错原文）。

- [ ] **Step 7: 提交**

```bash
git add crates/pocs/Cargo.toml crates/pocs/src/bin/poc4_windows.rs crates/pocs/src/poc4/
git commit -m "feat(pocs): poc4 Windows 设备层骨架(wintun L3)

Windows 支持评估 POC 第一块:tun-rs wintun 适配器封装与
--gate1-create/--gate1-ip 自验入口;依赖全部收敛在
cfg(windows) target 块,非 Windows 平台仅编译占位 main。"
```

---

### Task 5: Gate 1b（地址/路由应用 + 从设备读到包）

**Files:**
- Modify: `crates/pocs/src/poc4/device_win.rs`（gate1_cli 增加 `--gate1-read`）
- Create: `crates/pocs/src/poc4/routes_win.rs`（RouteApplier 的 Windows 版拷贝）
- Modify: `crates/pocs/src/poc4/mod.rs`（注册 `mod routes_win;`）

**Interfaces:**
- Consumes: Task 4 的 `WinDevice`。
- Produces: `routes_win::RouteApplier`（`new(if_index) -> io::Result<Self>`、`apply(&[RouteSpec]) -> usize`、`teardown()`；`RouteSpec { target: (IpAddr, u8), via: Option<IpAddr> }`），Task 7 复用。

- [ ] **Step 1: 写路由应用模块**

`crates/pocs/src/poc4/routes_win.rs`（语义与 zt-core routes.rs 相同：默认路由跳过、差分、单条失败告警继续）：

```rust
//! managed 路由差分应用(route_manager 的 Windows 后端,CreateIpForwardEntry2)。
//! 语义平移 zt-core/routes.rs:默认路由一律跳过,绝不劫持默认路由;
//! 单条失败告警继续;teardown 尽力清理。
use std::io;
use std::net::IpAddr;
use route_manager::{Route, RouteManager};

/// 与 zt-core config::ManagedRoute 对应的精简条目(metric 不参与增删判定)。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RouteSpec {
    pub target: IpAddr,
    pub prefix: u8,
    pub via: Option<IpAddr>,
}

fn is_default(target: &IpAddr, prefix: u8) -> bool {
    prefix == 0
}

pub struct RouteApplier {
    rm: RouteManager,
    if_index: u32,
    applied: Vec<RouteSpec>,
}

impl RouteApplier {
    pub fn new(if_index: u32) -> io::Result<Self> {
        Ok(Self { rm: RouteManager::new()?, if_index, applied: Vec::new() })
    }

    fn to_route(&self, r: &RouteSpec) -> Option<Route> {
        if is_default(&r.target, r.prefix) {
            return None;
        }
        let mut route = Route::new(r.target, r.prefix).with_if_index(self.if_index);
        if let Some(via) = r.via {
            route = route.with_gateway(via);
        }
        Some(route)
    }

    /// 差分应用;返回本次新安装条数。
    pub fn apply(&mut self, want: &[RouteSpec]) -> usize {
        let mut installed = 0;
        for r in want {
            if is_default(&r.target, r.prefix) || self.applied.contains(r) {
                continue;
            }
            let Some(route) = self.to_route(r) else { continue };
            if let Err(e) = self.rm.add(&route) {
                eprintln!("[routes] add {}/{} 失败(忽略): {}", r.target, r.prefix, e);
                continue;
            }
            self.applied.push(r.clone());
            installed += 1;
        }
        let stale: Vec<RouteSpec> =
            self.applied.iter().filter(|a| !want.contains(a)).cloned().collect();
        for r in &stale {
            self.remove(r);
        }
        installed
    }

    fn remove(&mut self, r: &RouteSpec) {
        if let Some(route) = self.to_route(r) {
            if let Err(e) = self.rm.delete(&route) {
                eprintln!("[routes] delete {}/{} 失败(忽略): {}", r.target, r.prefix, e);
            }
        }
        self.applied.retain(|a| a != r);
    }

    pub fn teardown(&mut self) {
        let all: Vec<RouteSpec> = self.applied.clone();
        for r in &all {
            self.remove(r);
        }
    }
}
```

- [ ] **Step 2: gate1_cli 增加 `--gate1-read <ip>/<prefix>`**

在 `device_win.rs` 的 `gate1_cli` match 增加分支（行为：建设备 → 配地址 → 起 reader 线程打印收到的每个包的摘要 → 60 秒后退出；同时提示从 Windows 侧 ping 网内另一地址触发发包）：

```rust
Some(a) if a == "--gate1-read" => {
    let ip_arg = args.get(2).expect("用法: --gate1-read a.b.c.d/nn").clone();
    let (ip, prefix) = ip_arg.split_once('/').expect("前缀用 / 分隔");
    let ip: std::net::Ipv4Addr = ip.parse().expect("非法 IPv4");
    let prefix: u8 = prefix.parse().expect("非法前缀");
    let d = match WinDevice::new("planet-port", 2800) {
        Ok(d) => d,
        Err(e) => { eprintln!("[gate1] FAIL {e}"); return 1; }
    };
    if let Err(e) = d.add_address_v4(ip, prefix) {
        eprintln!("[gate1] FAIL 地址应用失败: {e}");
        return 1;
    }
    // on-link 路由通常随地址自动生成;补一条对端测试网段路由,诱导内核发包进 ring
    let probe_net = std::net::Ipv4Addr::new(ip.octets()[0], ip.octets()[1], ip.octets()[2], 200);
    let mut ra = match routes_win::RouteApplier::new(d.if_index()) {
        Ok(r) => r,
        Err(e) => { eprintln!("[gate1] FAIL 路由应用器: {e}"); return 1; }
    };
    let n = ra.apply(&[routes_win::RouteSpec {
        target: probe_net.into(),
        prefix,
        via: None,
    }]);
    println!("[gate1] 地址 {ip}/{prefix} 已应用;探针路由安装 {n} 条({probe_net}/{prefix} on-link)");
    println!("[gate1] 60s 内请在另一终端执行: ping {}", probe_net);
    let d = std::sync::Arc::new(d);
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let rd = std::sync::Arc::clone(&d);
    let rd_stop = std::sync::Arc::clone(&stop);
    let reader = std::thread::spawn(move || {
        let mut buf = [0u8; 4000];
        let mut seen = 0u64;
        while !rd_stop.load(std::sync::atomic::Ordering::SeqCst) {
            match rd.recv(&mut buf) {
                Ok(len) => {
                    seen += 1;
                    // L3:首字节高 4 位是 IP 版本;wintun 理论只交付 IP 包
                    let ver = buf[0] >> 4;
                    let proto = if len >= 10 { buf[9] } else { 0 };
                    println!("[gate1] 包#{seen}: len={len} ipver={ver} proto={proto}(1=ICMP 17=UDP 6=TCP)");
                }
                Err(e) => {
                    eprintln!("[gate1] recv 错误(继续): {e}");
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            }
        }
        seen
    });
    std::thread::sleep(std::time::Duration::from_secs(60));
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    // reader 可能仍阻塞在 recv,给它 2s 收尾;超时就放弃 join(进程即将退出)
    let seen = match reader.join() {
        Ok(n) => n,
        Err(_) => {
            eprintln!("[gate1] reader 线程异常退出");
            0
        }
    };
    println!("[gate1] 60s 共读到 {seen} 个包(>0 即 Gate 1 读路径通过)");
    ra.teardown();
    if seen > 0 { 0 } else { 1 }
}
```

注意：`AtomicBool::new` / `load/store` 需要 `use std::sync::atomic::{AtomicBool, Ordering};`（就地把上面裸写 `std::sync::atomic::` 的地方收敛成 use 导入，与 crates 现有风格一致）。reader 线程若在 60 秒后仍阻塞在 recv，进程退出即可（Windows 进程终止不关心线程未 join；此处可接受，POC 代码）。

- [ ] **Step 3: mod.rs 注册模块并处理本 Gate 的验证矩阵**

`mod.rs` 增加 `mod routes_win;`。同时把 `--gate1-read` 中"读到包"的两种结果都视为**有效实测数据**：
- 读到 `ipver=4 proto=1` 的 ICMP 包 → Windows 直接把 IP 包送进 ring，符合 wintun L3 文档预期，Task 7 的帧桥按此实现；
- 读到非 IP 包（`ipver!=4`，可能就是 ARP）→ 记录原文十六进制前 14 字节，Task 7 需要在帧桥里补"应答 Windows 侧 ARP"分支；
- 0 个包 → 记录 `route print` 输出，检查探针路由是否生效；若路由在而包不进 ring，大概率是 Windows 邻居解析卡住（无人应答 ARP），此时启用备用方案：地址改配 /32（`--gate1-read a.b.c.d/32`）+ 对端探针地址用 /32 host 路由重试（wireguard-windows 的配法）。**无论哪种结果都写进 compatibility.md 草稿，这就是 Gate 1 的评估产出。**

- [ ] **Step 4: 同步、构建、实测**

```bash
./scripts/sync-win10.sh
ssh win10 "cmd /c \"cd /d C:\\planet-port && call C:\\BuildTools\\VC\\Auxiliary\\Build\\vcvars64.bat && set LIBCLANG_PATH=C:\\Program Files\\LLVM\\bin && cargo build -p pocs --bin poc4_windows\""
ssh win10 "C:\\planet-port\\target\\debug\\poc4_windows.exe --gate1-read 10.121.16.50/24"
```

（IP 用 planet 网内一段未被占用的测试前缀，避免与实网冲突；与用户确认选值。）
Expected: 60 秒窗口内配合另一终端 `ping 10.121.16.200` 读到包。退出码 0 = Gate 1 通过；否则按 Step 3 矩阵归因记录。

- [ ] **Step 5: mac 侧编译验证 + 提交**

```bash
cargo build --workspace
git add crates/pocs/src/poc4/
git commit -m "feat(pocs): poc4 Windows 地址路由应用与收包自验

Gate 1 收尾:managed 路由差分应用(route_manager Windows 后端)
与 --gate1-read 读路径自验;探针路由诱导内核发包进 wintun ring,
实测记录 Windows L3 直投/ARP 泄漏两种形态供数据面实现定案。"
```

---

### Task 6: Gate 2a（Windows 版 node 驱动：控制面打通，join planet 至 netconf OK）

**Files:**
- Modify: `crates/pocs/src/poc4/store.rs`（替换占位）
- Modify: `crates/pocs/src/poc4/sa.rs`（替换占位）
- Modify: `crates/pocs/src/poc4/netcfg.rs`（替换占位）
- Modify: `crates/pocs/src/poc4/driver.rs`（替换占位）
- Modify: `crates/pocs/src/poc4/mod.rs`（入口接 `--planet/--join` 流程）

**Interfaces:**
- Consumes: zt-sys 的 bindgen 绑定（Win10 实际生成产物为准）；Task 3 Step 3 记录的字段形态。
- Produces: `driver::PocNode`（`start(data_dir, planet_file, bind_port, sink) -> Result<PocNode, String>`、`join(nwid)`、`address()`、`online()`、`networks() -> Vec<netcfg::NetConfig>`、`frame_in_tx()`、`shutdown(self)`）与 trait `driver::PocSink`（`on_event(&str)`、`on_net_config(NetConfig)`、`on_frame(nwid, src, dst, eth, payload)`、`local_ipv4s() -> Vec<Ipv4Addr>`）——Task 7 的数据面直接挂上去。

- [ ] **Step 1: store.rs（StateStore，无 0600）**

整文件平移 `crates/zt-core/src/state.rs`，仅两处改动：删掉 `use std::os::unix::fs::PermissionsExt;` 与 `fs::set_permissions` 那两行（Windows 侧 POC 不做 ACL，文件头注释写明"0600 收敛留正式实现"）。`zt_obj` 枚举与 `get/put` 签名保持原样。

- [ ] **Step 2: sa.rs（winsock 布局）**

```rust
//! sockaddr_storage <-> SocketAddr(winsock 布局:无 sin_len,in6_addr 是 u.Byte)。
//! 语义与 zt-core/sa.rs 相同;差异点即 Windows 适配面,见 Task 3 Step 3 实测记录。
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use zt_sys as zt;

fn in6_bytes(a: &zt::in6_addr) -> [u8; 16] {
    unsafe { a.u.Byte }
}
fn set_in6_bytes(dst: &mut zt::in6_addr, b: [u8; 16]) {
    unsafe { dst.u.Byte = b }
}

/// # Safety
/// `ss` 为 NULL 或指向核心提供的有效 sockaddr_storage。
pub unsafe fn from_sockaddr_storage(ss: *const zt::sockaddr_storage) -> Option<SocketAddr> {
    if ss.is_null() {
        return None;
    }
    unsafe {
        match (*ss).ss_family as u32 {
            zt::AF_INET => {
                let a = &*(ss as *const zt::sockaddr_in);
                Some(SocketAddr::new(
                    IpAddr::V4(Ipv4Addr::from(a.sin_addr.s_addr.to_ne_bytes())),
                    u16::from_be(a.sin_port),
                ))
            }
            zt::AF_INET6 => {
                let a = &*(ss as *const zt::sockaddr_in6);
                Some(SocketAddr::new(
                    IpAddr::V6(Ipv6Addr::from(in6_bytes(&a.sin6_addr))),
                    u16::from_be(a.sin6_port),
                ))
            }
            _ => None,
        }
    }
}

/// `port_override`:收发物理包传真实端口;网络条目传前缀长度(核心编码在 sin_port)。
pub fn to_sockaddr_storage(addr: &SocketAddr, port_override: Option<u16>) -> zt::sockaddr_storage {
    let mut ss: zt::sockaddr_storage = unsafe { std::mem::zeroed() };
    let port = port_override.unwrap_or(addr.port()).to_be();
    match addr.ip() {
        IpAddr::V4(v4) => unsafe {
            let a = &mut *(&mut ss as *mut _ as *mut zt::sockaddr_in);
            a.sin_family = zt::AF_INET as _;
            a.sin_port = port;
            a.sin_addr.s_addr = u32::from_ne_bytes(v4.octets());
        },
        IpAddr::V6(v6) => unsafe {
            let a = &mut *(&mut ss as *mut _ as *mut zt::sockaddr_in6);
            a.sin6_family = zt::AF_INET6 as _;
            a.sin6_port = port;
            set_in6_bytes(&mut a.sin6_addr, v6.octets());
        },
    }
    ss
}
```

若 Task 3 Step 3 实测的字段名与上述不同（如 `AF_INET` 是 `i32`、in6 union 名不同），就按 bindings.rs 实际形态改本文件，**其余逻辑不动**。

- [ ] **Step 3: netcfg.rs（NetConfig 拷贝，精简版）**

平移 `crates/zt-core/src/config.rs` 的 `NetConfig`（去掉 daemon 用的 dns_domain/dns_servers 字段亦可，但**保留** nwid/name/mac/mtu/netconf_revision/assigned/routes/status/is_private——Task 7 全要用）：

```rust
//! ZT_VirtualNetworkConfig 的完整拷贝(回调内指针不保证存活,必须立即拷)。
//! 字段语义与 zt-core/config.rs 一致,这里只裁掉 POC 不用的 DNS 字段。
use std::net::IpAddr;
use crate::sa;
use zt_sys as zt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpNet {
    pub addr: IpAddr,
    pub prefix: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedRoute {
    pub target: IpNet,
    pub via: Option<IpAddr>,
    pub metric: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetStatus {
    RequestingConfiguration,
    Ok,
    AccessDenied,
    NotFound,
    PortError,
    ClientTooOld,
    AuthenticationRequired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetConfig {
    pub nwid: u64,
    pub name: String,
    pub mac: u64,
    pub status: NetStatus,
    pub is_private: bool,
    pub mtu: u16,
    pub netconf_revision: u64,
    pub assigned: Vec<IpNet>,
    pub routes: Vec<ManagedRoute>,
}

/// 前缀长度编码在 sin_port(核心 InetAddress 以 port 字段存 netmaskBits)。
unsafe fn ip_net_of(ss: &zt::sockaddr_storage) -> Option<IpNet> {
    let sa = sa::from_sockaddr_storage(ss)?;
    Some(IpNet { addr: sa.ip(), prefix: sa.port() as u8 })
}

impl NetConfig {
    /// # Safety
    /// `c` 指向核心已初始化的完整 ZT_VirtualNetworkConfig。
    pub unsafe fn from_zt(c: &zt::ZT_VirtualNetworkConfig) -> Self {
        use std::ffi::CStr;
        use zt::ZT_VirtualNetworkStatus_ZT_NETWORK_STATUS_ACCESS_DENIED as ACCESS_DENIED;
        use zt::ZT_VirtualNetworkStatus_ZT_NETWORK_STATUS_AUTHENTICATION_REQUIRED as AUTH_REQUIRED;
        use zt::ZT_VirtualNetworkStatus_ZT_NETWORK_STATUS_CLIENT_TOO_OLD as CLIENT_TOO_OLD;
        use zt::ZT_VirtualNetworkStatus_ZT_NETWORK_STATUS_NOT_FOUND as NOT_FOUND;
        use zt::ZT_VirtualNetworkStatus_ZT_NETWORK_STATUS_OK as OK;
        use zt::ZT_VirtualNetworkStatus_ZT_NETWORK_STATUS_PORT_ERROR as PORT_ERROR;
        use zt::ZT_VirtualNetworkStatus_ZT_NETWORK_STATUS_REQUESTING_CONFIGURATION as REQUESTING;
        let status = match c.status {
            REQUESTING => NetStatus::RequestingConfiguration,
            OK => NetStatus::Ok,
            ACCESS_DENIED => NetStatus::AccessDenied,
            NOT_FOUND => NetStatus::NotFound,
            PORT_ERROR => NetStatus::PortError,
            CLIENT_TOO_OLD => NetStatus::ClientTooOld,
            AUTH_REQUIRED => NetStatus::AuthenticationRequired,
            _ => NetStatus::RequestingConfiguration,
        };
        let name = CStr::from_ptr(c.name.as_ptr()).to_string_lossy().into_owned();
        let n_assigned = (c.assignedAddressCount as usize).min(c.assignedAddresses.len());
        let assigned = c.assignedAddresses[..n_assigned].iter().filter_map(|ss| ip_net_of(ss)).collect();
        let n_routes = (c.routeCount as usize).min(c.routes.len());
        let routes = c.routes[..n_routes]
            .iter()
            .filter_map(|r| {
                Some(ManagedRoute {
                    target: ip_net_of(&r.target)?,
                    via: sa::from_sockaddr_storage(&r.via).map(|a| a.ip()),
                    metric: r.metric,
                })
            })
            .collect();
        Self {
            nwid: c.nwid,
            name,
            mac: c.mac,
            status,
            is_private: c.type_ == zt::ZT_VirtualNetworkType_ZT_NETWORK_TYPE_PRIVATE, // PRIVATE=0
            mtu: c.mtu as u16,
            netconf_revision: c.netconfRevision,
            assigned,
            routes,
        }
    }
}
```

（上面 `is_private` 若要保留则给 NetConfig 加 `pub is_private: bool` 字段——加上，poc3 输出风格里有。）

- [ ] **Step 4: driver.rs（Windows 版驱动，channel 替代 pipe/poll）**

完整平移 `crates/zt-core/src/node.rs` 的架构（回调 glue 只做拷贝 + channel send；全部 sink 调用发生在驱动线程且 catch_unwind 包裹；shutdown 硬顺序 running=false → wake → Cmd::Stop → join → ZT_Node_delete），以下四点为 Windows 改造点，其余逐行保留：

1. **UDP 拆两个线程**：`Inner.udp` 只留给 `wire_send`（`try_clone` 出发送端）；独立的读线程阻塞 `recv_from`，把 `(Vec<u8>, SocketAddr)` 发进 `udp_rx` 通道；驱动线程从 `udp_rx` 消费并调 `ZT_Node_processWirePacket`（local_socket 参数传 `sock.as_raw_socket() as i64`，winsock SOCKET）。
2. **唤醒机制**：删 `make_pipe`/`wake_rd`/`wake_wr`；`Inner` 持 `wake_tx: Sender<u8>`，`wake()` 即 `let _ = self.wake_tx.send(1);`；驱动线程循环尾部 `d.wake_rx.recv_timeout(tick)`，`tick = ((d.deadline - now).clamp(1, 500))ms`——超时与收到唤醒都进入下一圈（等价原 poll 语义）。
3. **imports**：`std::os::fd::*` → `std::os::windows::io::AsRawSocket`；`libc::*` 全部移除。
4. **多播订阅 ADI**：`u32::from(*ip) as libc::c_ulong` → `u32::from(*ip) as u64`（bindgen 的 multicastSubscribe ADI 形参按 bindings.rs 实际类型对齐，通常 `u64`）。

对外类型（Task 7 依赖）：

```rust
pub trait PocSink: Send + 'static {
    fn on_event(&mut self, ev: &str);
    fn on_net_config(&mut self, cfg: netcfg::NetConfig);
    fn on_frame(&mut self, nwid: u64, src_mac: u64, dst_mac: u64, ether_type: u16, payload: &[u8]);
    fn local_ipv4s(&self) -> Vec<Ipv4Addr>;
}

pub struct PocNode { /* node/inner/driver/shut_down 同 zt-core NodeHandle */ }
impl PocNode {
    pub fn start(data_dir: PathBuf, planet_file: PathBuf, bind_port: u16, sink: impl PocSink) -> Result<Self, String>;
    pub fn join(&self, nwid: u64) -> Result<(), String>;
    pub fn address(&self) -> u64;
    pub fn online(&self) -> bool;
    pub fn networks(&self) -> Vec<netcfg::NetConfig>;
    /// 设备→核心帧通道(数据面读线程用;发送后自动唤醒驱动线程)
    pub fn frame_in_tx(&self) -> Sender<FrameIn>;
    pub fn shutdown(self);
}
pub struct FrameIn { pub nwid: u64, pub src: u64, pub dst: u64, pub eth: u16, pub payload: Vec<u8> }
```

planet 注入、world_id 解析、`ZT_Node_new` 前置校验、首次 `processBackgroundTasks`、`map_state_obj`、六个回调 glue、`run_driver` 循环骨架（命令/事件/帧/netcfg 消费、组播订阅 5000ms 扫描、POLLERR 分支删除）全部按 node.rs 原样平移——**这是平移不是重写，遇到与 Unix 原型语义分叉时以原型为准**。

- [ ] **Step 5: mod.rs 入口接控制面流程**

`crate_main` 改为：无参数时打印用法；`--planet <file> --join <nwid> --dir <dir> [--port 9994] [--duration 600] [--target ip]` 走控制面：start → join → 每 2s 打印 address/online/netconf 状态（对齐 poc3 的 print_status 风格）→ duration 到点 shutdown。sink 的 `on_net_config` 先只打印（设备应用留 Task 7）。

- [ ] **Step 6: 同步、构建、实测（控制面）**

```bash
./scripts/sync-win10.sh
ssh win10 "cmd /c \"cd /d C:\\planet-port && call C:\\BuildTools\\VC\\Auxiliary\\Build\\vcvars64.bat && set LIBCLANG_PATH=C:\\Program Files\\LLVM\\bin && cargo build -p pocs --bin poc4_windows\""
ssh win10 "C:\\planet-port\\target\\debug\\poc4_windows.exe --planet C:\\planet-port\\crates\\pocs\\fixtures\\planet.Gozi25 --join 5cf54f181554090f --dir C:\\planet-port\\target\\poc-data\\win-a --port 9994 --duration 120"
```

Expected 时间线：`address=xxxxx` → `online=true`（1-30s，Gozi25 根对新建会话有 UDP 过滤的历史问题——见 memory，若 9994 不通换 9993 试）→ netconf `REQUESTING_CONFIGURATION`；此时**需要用户在控制器授权新节点**，随后 `status=OK assigned=[10.x.x.x/..]`。授权前阻塞是预期行为，等待用户操作。
失败归因：identity 落盘失败看 store 路径权限；wire 通不通看 peers 输出（driver 可加 peers 打印，对齐 poc3）。

- [ ] **Step 7: mac 编译验证 + 提交**

```bash
cargo build --workspace
git add crates/pocs/src/poc4/
git commit -m "feat(pocs): poc4 Windows 版 ZeroTier node 驱动(控制面)

平移 zt-core node.rs 架构,Windows 改造点收敛为三处:UDP 独立
读线程替代 fd poll、mpsc 唤醒替代 pipe、winsock 布局的 sockaddr
转换;回调 glue/驱动线程红线语义与原型一致。join planet 至
netconf OK 的控制面在 Win10 实测通过。"
```

---

### Task 7: Gate 2b（数据面：wintun↔核心帧桥 + ARP 垫层，互 ping）

**Files:**
- Modify: `crates/pocs/src/poc4/arp.rs`（替换占位）
- Modify: `crates/pocs/src/poc4/mod.rs`（sink 接设备应用与帧桥）

**Interfaces:**
- Consumes: Task 5 的 `WinDevice`/`RouteApplier`、Task 6 的 `PocNode`/`PocSink`/`FrameIn`。
- Produces: **Gate 2 结论**（互 ping 实录）。

- [ ] **Step 1: arp.rs（IPv4 ARP 垫层 + 邻居学习）**

```rust
//! IPv4 ARP 垫层:wintun(L3)与 ZeroTier 核心(L2)之间的桥接层。
//!
//! 职责(POC 范围 IPv4):
//! 1. 应答远端 peer 的 ARP request(问我们的 managed IP → 以本网 MAC 回 reply);
//! 2. 出向(Windows IP 包 → 核心帧):维护 IP→MAC 学习表,来源是入向帧的
//!    (以太网 src MAC, IP src)绑定;查不到时用广播 MAC 直发(POC 策略,
//!    不做请求-缓存队列)。
//! 3. 入向 ARP reply(对我们在 Task 7b 主动发出的 probe request 的应答)入表。
pub mod ether {
    pub const ETHERTYPE_IPV4: u16 = 0x0800;
    pub const ETHERTYPE_ARP: u16 = 0x0806;
    pub const BROADCAST: u64 = 0xffff_ffff_ffff;
    pub fn mac_bytes(m: u64) -> [u8; 6] { m.to_be_bytes()[2..].try_into().unwrap() }
    pub fn mac_from_bytes(b: &[u8]) -> u64 {
        let mut wid = [0u8; 8];
        wid[2..].copy_from_slice(&b[..6]);
        u64::from_be_bytes(wid)
    }
}

pub struct ArpShim {
    /// 本网虚拟 MAC(核心分配的 cfg.mac)
    pub local_mac: u64,
    /// 需要应答 ARP 的本机 managed IPv4
    pub our_ips: Vec<std::net::Ipv4Addr>,
    /// 学习表:IP → MAC
    table: std::collections::HashMap<std::net::Ipv4Addr, u64>,
}

impl ArpShim {
    pub fn new(local_mac: u64) -> Self {
        Self { local_mac, our_ips: Vec::new(), table: Default::default() }
    }
    pub fn set_ips(&mut self, ips: Vec<std::net::Ipv4Addr>) { self.our_ips = ips; }

    /// 出向:Windows 裸 IP 包 → (dst_mac, src_mac, ethertype, payload)。
    /// src IP 来自 IP 头(20 字节起);dst 查表,miss 用广播。
    pub fn wrap_outbound(&mut self, pkt: &[u8]) -> Option<(u64, u64, u16, Vec<u8>)> {
        if pkt.len() < 20 || pkt[0] >> 4 != 4 {
            return None; // POC 只桥 IPv4
        }
        let src_ip = std::net::Ipv4Addr::new(pkt[12], pkt[13], pkt[14], pkt[15]);
        let dst_ip = std::net::Ipv4Addr::new(pkt[16], pkt[17], pkt[18], pkt[19]);
        // 免费自学习:自己发的包也把 src 绑定记下(对端后续 ARP 我们时可命中的是对端表,这里其实学自己,无害)
        let dst = self.table.get(&dst_ip).copied().unwrap_or(ether::BROADCAST);
        let _ = src_ip;
        Some((dst, self.local_mac, ether::ETHERTYPE_IPV4, pkt.to_vec()))
    }

    /// 入向帧学习:IP 帧的 (src MAC, src IP) 绑定入表。
    pub fn learn(&mut self, src_mac: u64, ether_type: u16, payload: &[u8]) {
        if ether_type != ether::ETHERTYPE_IPV4 || payload.len() < 20 {
            return;
        }
        let src_ip = std::net::Ipv4Addr::new(payload[12], payload[13], payload[14], payload[15]);
        self.table.insert(src_ip, src_mac);
    }

    /// 处理核心送来的 ARP 帧:问 our_ips 且是 request → 构造 reply payload。
    /// 返回 Some(reply_payload) 时,调用方以 (tha=req.sha, src=local_mac, ARP) 喂回核心。
    pub fn answer(&mut self, payload: &[u8]) -> Option<(u64, Vec<u8>)> {
        if payload.len() < 28 {
            return None;
        }
        let htype = u16::from_be_bytes([payload[0], payload[1]]);
        let ptype = u16::from_be_bytes([payload[2], payload[3]]);
        if htype != 1 || ptype != ether::ETHERTYPE_IPV4 {
            return None;
        }
        let oper = u16::from_be_bytes([payload[6], payload[7]]);
        let sha = ether::mac_from_bytes(&payload[8..14]);
        let spa = std::net::Ipv4Addr::new(payload[14], payload[15], payload[16], payload[17]);
        let tpa = std::net::Ipv4Addr::new(payload[24], payload[25], payload[26], payload[27]);
        match oper {
            1 => {
                // request:问的是不是我们的 IP
                if !self.our_ips.contains(&tpa) {
                    return None;
                }
                let mut r = [0u8; 28];
                r[0..2].copy_from_slice(&htype.to_be_bytes());
                r[2..4].copy_from_slice(&ptype.to_be_bytes());
                r[4] = 6; // hlen
                r[5] = 4; // plen
                r[6..8].copy_from_slice(&2u16.to_be_bytes()); // reply
                r[8..14].copy_from_slice(&ether::mac_bytes(self.local_mac)); // sha
                r[14..18].copy_from_slice(&tpa.octets()); // spa = 被问的 IP
                r[18..24].copy_from_slice(&ether::mac_bytes(sha)); // tha = 请求者
                r[24..28].copy_from_slice(&spa.octets()); // tpa = 请求者 IP
                // 顺带学习请求者绑定
                self.table.insert(spa, sha);
                Some((sha, r.to_vec()))
            }
            2 => {
                // reply(对我们 probe 的应答):学习 spa↔sha
                self.table.insert(spa, sha);
                None
            }
            _ => None,
        }
    }
}
```

（ARP 无校验和，不要往代码里加 checksum 相关逻辑。）

- [ ] **Step 2: mod.rs 组装数据面**

`crate_main` 的完整模式下，sink 与设备整合为 `Plane` 结构（设备 + RouteApplier + ArpShim + frame_in_tx 的 OnceLock 槽，对齐 poc3 的 Sink/DevicePlane 协作方式）：

- `on_net_config(Up|ConfigUpdate)`：建/取 `WinDevice`（mtu=max(cfg.mtu,1280)，设备名 `planet-port`）→ status==Ok 时 `add_address_v4` 差分 + `RouteApplier::apply(cfg.routes→RouteSpec)` + `ArpShim.our_ips` 更新；
- `on_frame`：`learn(src_mac, eth, payload)`；eth==ARP → `answer()` 有则回喂（以 `frame_in_tx` 发 `FrameIn{dst:请求者MAC, src:local_mac, eth:ARP}`）；eth==IPv4 → `WinDevice::send(payload)`（裸 IP 写 ring）；
- 设备读线程：`recv` → `ArpShim::wrap_outbound` → `frame_in_tx` 发 `FrameIn{dst, src, eth:0x0800, payload}`；
- `local_ipv4s`：物理口 IPv4（`GetAdaptersAddresses` 不引入了——POC 简化为返回 `WinDevice` 已应用的 managed IPv4,对齐 poc3 的 `d.applied_v4s()` 思路;注释说明物理口枚举留正式实现,组播订阅覆盖面收窄不影响互通验证）。

主循环：每 2s 打印状态（对齐 poc3 `print_status`），每 5s `ping -n 1 -w 2000 <target>`（Windows ping 参数,替换 poc3 的 `/sbin/ping`）；`--target` 给 mac/linux 网内节点（如 10.121.16.229）。关停顺序红线：设备读线程 stop → RouteApplier teardown → drop 设备 → `PocNode::shutdown`。

- [ ] **Step 3: 同步、构建、实测（Gate 2）**

```bash
./scripts/sync-win10.sh
ssh win10 "cmd /c \"cd /d C:\\planet-port && call C:\\BuildTools\\VC\\Auxiliary\\Build\\vcvars64.bat && set LIBCLANG_PATH=C:\\Program Files\\LLVM\\bin && cargo build -p pocs --bin poc4_windows\""
ssh win10 "C:\\planet-port\\target\\debug\\poc4_windows.exe --planet C:\\planet-port\\crates\\pocs\\fixtures\\planet.Gozi25 --join 5cf54f181554090f --dir C:\\planet-port\\target\\poc-data\\win-a --port 9994 --duration 300 --target <mac侧节点IP>"
```

Expected：netconf OK 后 mac 侧 `ping <win10 assigned IP>` 双向通（mac 侧由用户或另开会话执行）；win10 侧周期 ping target 出现 `通`。同时反向验证：mac 侧先 ping 触发 ARP（观察 win10 侧 `answer` 命中日志）。
失败归因顺序：① 看 ARP 垫层是否收到 request（无 → 检查 mac 侧ARP是否到达 on_frame）;② wrap_outbound 的 dst MAC 学习是否命中（miss 走广播是否被对端收）;③ route print 确认 managed 路由;④ peers 输出确认 P2P 路径(可能经 planet 中继,亦算通)。
**双向互 ping 通 = Gate 2 通过；只有单向通也要记录方向与归因。**

- [ ] **Step 4: mac 编译验证 + 提交**

```bash
cargo build --workspace
git add crates/pocs/src/poc4/
git commit -m "feat(pocs): poc4 Windows 数据面(wintun 帧桥 + IPv4 ARP 垫层)

Gate 2 收尾:Windows 裸 IP 包补以太网头经 ARP 学习表定目的 MAC
喂 ZeroTier 核心,核心 ARP request 以 managed IP 应答;Win10 与
mac/linux 节点互 ping 实测,ARP 垫层实测代价与结论记录于
compatibility.md。"
```

---

### Task 8: 评估产出落盘（compatibility.md + 结论 + 零影响终验）

**Files:**
- Modify: `docs/compatibility.md`
- Create: `docs/2026-10-31-windows-poc-report.md`（评估报告）

**Interfaces:**
- Consumes: Task 1-7 全部实测记录。
- Produces: 评估最终交付物。

- [ ] **Step 1: 写评估报告**

`docs/2026-10-31-windows-poc-report.md` 按规格"评估产出"节组织：
- 环境实测（Win10 版本/build/arch、工具链版本、HVCI/签名策略状态 `powershell -Command "Get-CimInstance -ClassName Win32_DeviceGuard -Namespace root\Microsoft\Windows\DeviceGuard"`）；
- 三道 Gate 各自：结果（PASS/FAIL）、关键实测数据（Gate 1 的 ring 包形态、Gate 2 的互 ping 方向与延迟、ARP 垫层命中数）、过程中踩的坑与归因；
- wintun 路线结论（确认/否决 + ARP 垫层实测代价）；
- 正式 Windows 支持的工作量清单（基于实测修订规格调研报告的第 7 节：哪些面已被 POC 验证、哪些仍纸面——服务化 windows.rs、DNS NRPT、node.rs cfg 改造回迁、release.sh 目标、签名）。

- [ ] **Step 2: compatibility.md 加 Win10 行**

对齐现有 macOS/Linux 实测条目格式（先读该文件确认格式再写），补 Win10 的版本/架构/Gate 结果行。

- [ ] **Step 3: 零影响终验**

```bash
cargo build --workspace && cargo build --workspace --release 2>/dev/null || cargo build --workspace
git --no-pager log --oneline -8
git --no-pager diff db7a09a..HEAD --stat -- crates/zt-core crates/daemon crates/cli crates/api scripts/release.sh
```

Expected: 构建成功;diff 输出为空（既有 crate 与发布脚本零改动,隔离红线达成）。

- [ ] **Step 4: 提交**

```bash
git add docs/compatibility.md docs/2026-10-31-windows-poc-report.md
git commit -m "docs: Windows 支持评估实测记录与结论

Win10 实机三道 Gate 实测(构建链路/wintun 设备层/组网互通),
wintun L3 路线与 ARP 垫层实测代价、正式支持工作量清单。"
```

---

## Self-Review 记录

- 规格覆盖：环境准备（Task 1-2）、Gate 0（Task 3）、Gate 1（Task 4-5）、Gate 2（Task 6-7）、产出与风险记录（Task 8）——规格各节均有对应任务。
- 占位符扫描：Task 6 Step 1/4 的"平移"指向确切源文件与改造点清单（非 TBD）；`<mac侧节点IP>` 等待执行时由用户提供的运行参数,非代码缺口。
- 类型一致性：`WinDevice`（Task 4 定义，Task 5/7 使用）、`RouteSpec/RouteApplier`（Task 5 定义，Task 7 使用）、`PocNode/PocSink/FrameIn/NetConfig`（Task 6 定义，Task 7 使用）签名已核对一致。
