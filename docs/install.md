# planet-port 安装指南

适用平台:macOS(Apple Silicon / Intel)、Linux(amd64 / arm64,glibc≥2.31,
Ubuntu 20.04+ / Mint 21+ 等主流发行版,需 systemd)。Windows 在发布计划中。

安装后的系统落点(所有安装方式一致,按平台区分):

**macOS**

| 路径 | 内容 |
|---|---|
| `/Library/Application Support/planet-portd/bin/planet-port` | CLI |
| `/Library/Application Support/planet-portd/bin/planet-portd` | daemon |
| `/usr/local/bin/planet-port` | 指向 CLI 的软链(PATH 可见) |
| `/Library/LaunchDaemons/cn.imgozi.d.planetport.daemon.plist` | launchd 服务(开机自启、崩溃自动拉起) |
| `/Library/Logs/planet-portd/daemon.log` | 日志(newsyslog 5MB 轮转) |

**Linux**

| 路径 | 内容 |
|---|---|
| `/usr/local/bin/planet-port` | CLI(直接落 PATH 规范位,无软链层) |
| `/usr/local/bin/planet-portd` | daemon |
| `/etc/systemd/system/planet-portd.service` | systemd 服务(开机自启、异常退出自动拉起) |
| `/var/lib/planet-portd` | 数据目录(unit 显式 `--config-dir`) |
| `journalctl -u planet-portd` | 日志(journald,无需轮转配置) |

## 在线安装(推荐)

有外网的机器,一条命令:

```bash
curl -fsSL https://raw.githubusercontent.com/planet-port/planet-port-desktop/main/scripts/install.sh | sudo bash
```

脚本行为:识别平台架构 → 从 GitHub Releases 下载对应
`planet-port_<ver>_<platform>_<arch>.tar.gz`(macOS:`macos_aarch64` /
`macos_x86_64`;Linux:`linux_amd64` / `linux_arm64`)→ 解压 → 调用包内
`planet-port install` 完成安装(见下;macOS 装 launchd 服务,Linux 装
systemd 服务)。

指定版本 / 跳过服务:

```bash
curl -fsSL …/install.sh | sudo bash -s -- v0.1.1        # 指定版本
curl -fsSL …/install.sh | sudo bash -s -- --no-service  # 只落盘二进制(macOS 含软链),不装服务
```

## 离线安装

**方式一:`planet-port install`(推荐,无任何依赖)**

把与目标机器平台匹配的发布包 tar.gz 拷过去(U 盘/内网传输),解压后:

```bash
# macOS(Apple Silicon)
tar -xzf planet-port_0.1.0_macos_aarch64.tar.gz
sudo ./planet-port install

# Linux(amd64;arm64 换 linux_arm64 包)
tar -xzf planet-port_0.1.0_linux_amd64.tar.gz
sudo ./planet-port install
```

一步完成:同目录的 planet-port / planet-portd 落盘预设路径 → 装
平台服务(macOS 建 `/usr/local/bin/planet-port` 软链 + 写 plist 装载
launchd 服务;Linux 直接落 `/usr/local/bin/` + 写 unit,均立即启动、
随开机自启)。全程无网络、无脚本依赖。

**方式二:经 install.sh 的本地包入口**

目标机上已有 install.sh(如随发布包分发)时:

```bash
sudo PLANET_PORT_TARBALL=./planet-port_0.1.0_linux_amd64.tar.gz ./install.sh
```

与在线安装等价,仅"下载"一步替换为本地包。

## `install` 命令细节

- **需 root**(`sudo`),幂等:与已装版本相同时直接重装,不产生重复项、不提示;
- **版本差异确认**:已装版本与本次不同(无论升降级)时先提示
  `已安装 0.1.0,本次将安装 0.1.2(升级/降级),继续? [y/N]`,回车外的输入
  即取消(退出码 130);脚本场景加 `--yes` 跳过,`-j` 下版本不同必须配
  `--yes`(否则退出码 2)——降级同样走这条路,快速回滚就是装旧版包时答 y;
- 二进制替换用「临时文件 + rename」原子方式,**升级时无需先停服务**;
- **升级兜底**:安装前记录正在运行的 planet,新 daemon 拉起后自动恢复——
  覆盖"旧版 daemon 被 SIGTERM 后落 stopped"的升级场景,新版幂等;
- `-j` 输出:进度走 stderr,stdout 仅 `{"result":"ok"}`。

**Linux 与 macOS 的差异**

- **无软链层**:CLI / daemon 直接落 `/usr/local/bin/`(PATH 规范位),
  macOS 的「App Support/bin + `/usr/local/bin` 软链」两级结构不存在;
- **服务语义**:systemd unit 用 `Restart=on-failure` +
  `RestartSec=2`,对应 macOS 的 `KeepAlive={SuccessfulExit=false}`——
  异常退出自动拉起,SIGTERM 优雅退出 / `systemctl stop` 不拉起;
  enable 即开机自启,对应 `RunAtLoad=true`;
- **日志走 journald**:unit 不写文件日志,无轮转配置,`journalctl -u
  planet-portd` 查看,`service logs` 在 Linux 亦转 journalctl;
- **启动超时提示**:install / `service start` 等 15s 未在线时,
  提示改为 `journalctl -u planet-portd` 排查;
- **`service status` 不在线诊断**:unit 文件不存在 = 未安装;存在则
  给 `systemctl is-active` 状态摘要(替代 macOS 的 launchctl 摘要),
  退出码同为 3。

## 开发机自装

从源码构建后直接安装(等价离线方式一,源即 `target/release/`):

```bash
cargo build --release -p planet-port -p daemon
sudo ./target/release/planet-port install
```

## 升级与卸载

- **升级**:重复执行任一安装方式即可(数据目录与 planet 登记保留,daemon 按
  registry 期望状态自动恢复);
- **卸载**:

```bash
sudo planet-port service uninstall   # 停服务、删二进制/软链/plist/unit;数据目录保留

# 彻底清除(可选):
sudo rm -rf "/Library/Application Support/planet-portd"   # macOS
sudo rm -rf /var/lib/planet-portd                          # Linux
```

## 验证

```bash
planet-port service status   # 运行中(端口 9998);未安装/未运行时退出码 3
planet-port info             # planet 聚合状态
journalctl -u planet-portd   # Linux 下查日志
```

---

## 附:安装程序(.pkg)说明

当前阶段**只做在线 / 离线两条安装路径**,不做 .pkg。技术上可行(payload 即
现有 tarball + postinstall 调 `planet-port install`),待面向普通用户分发时
再评估;届时需同步解决 Developer ID 签名 + 公证,否则 Gatekeeper 会拦截
首次运行。
