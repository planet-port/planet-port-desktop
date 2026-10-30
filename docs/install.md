# planet-port 安装指南

适用平台:macOS(Apple Silicon / Intel)。Linux 与 Windows 在发布计划中。

安装后的系统落点(所有方式一致):

| 路径 | 内容 |
|---|---|
| `/Library/Application Support/planet-portd/bin/planet-port` | CLI |
| `/Library/Application Support/planet-portd/bin/planet-portd` | daemon |
| `/usr/local/bin/planet-port` | 指向 CLI 的软链(PATH 可见) |
| `/Library/LaunchDaemons/cn.imgozi.d.planetport.daemon.plist` | launchd 服务(开机自启、崩溃自动拉起) |
| `/Library/Logs/planet-portd/daemon.log` | 日志(newsyslog 5MB 轮转) |

## 在线安装(推荐)

有外网的机器,一条命令:

```bash
curl -fsSL https://raw.githubusercontent.com/planet-port/planet-port-desktop/main/scripts/install.sh | sudo bash
```

脚本行为:识别平台架构 → 从 GitHub Releases 下载对应
`planet-port_<ver>_<platform>_<arch>.tar.gz` → 解压 → 调用包内
`planet-port install` 完成安装(见下)。

指定版本 / 跳过服务:

```bash
curl -fsSL …/install.sh | sudo bash -s -- v0.1.1        # 指定版本
curl -fsSL …/install.sh | sudo bash -s -- --no-service  # 只装二进制与软链,不装服务
```

## 离线安装

**方式一:`planet-port install`(推荐,无任何依赖)**

把发布包 tar.gz 拷到目标机器(U 盘/内网传输),解压后:

```bash
tar -xzf planet-port_0.1.0_macos_aarch64.tar.gz
sudo ./planet-port install
```

一步完成:同目录的 planet-port / planet-portd 落盘预设路径 → 建
`/usr/local/bin/planet-port` 软链 → 写 plist 并装载 launchd 服务(立即启动、
随开机自启)。全程无网络、无脚本依赖。

**方式二:经 install.sh 的本地包入口**

目标机上已有 install.sh(如随发布包分发)时:

```bash
sudo PLANET_PORT_TARBALL=./planet-port_0.1.0_macos_aarch64.tar.gz ./install.sh
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
sudo planet-port service uninstall   # 停服务、删二进制/软链/plist;数据目录保留
sudo rm -rf "/Library/Application Support/planet-portd"   # 彻底清除(可选)
```

## 验证

```bash
planet-port service status   # 运行中(端口 9998);未安装/未运行时退出码 3
planet-port info             # planet 聚合状态
```

---

## 附:安装程序(.pkg)说明

当前阶段**只做在线 / 离线两条安装路径**,不做 .pkg。技术上可行(payload 即
现有 tarball + postinstall 调 `planet-port install`),待面向普通用户分发时
再评估;届时需同步解决 Developer ID 签名 + 公证,否则 Gatekeeper 会拦截
首次运行。
