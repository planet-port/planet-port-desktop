# 平台兼容性实测记录

每在一台新机型的实机上完成安装验证后,在下方表格补一行。发布矩阵见 docs/install.md ,
此表只记**实测过的机器**——未实测平台仅作占位跟踪、不算通过,发布产物支持但未实测的平台同样不算通过。

| 测试日期 | 机型 / 主机 | 系统 | 架构 | 验证结果 | 备注 |
|---|---|---|---|---|---|
| 2026-10-31 | MacBook Pro 16" (MacBookPro18,4) | macOS 26.6.2 | arm64 | 通过 | 开发机,daemon 常驻;launchd 路径 |
| 2026-10-31 | gozi-mba (10.152.124.24) | Linux Mint 21.3 (glibc 2.35) | x86_64 | 通过 | systemd 全流程 e2e;与官方 zerotier-one 共存 |
| 2026-10-31 | WX-RK3566 (10.121.15.88) | Ubuntu 20.04.6 LTS (glibc 2.31) | aarch64 | 通过 | systemd 全流程 e2e;与官方 zerotier-one 共存;glibc 基线机型 |
| 2026-10-31 | openclaw (192.168.8.220) | Ubuntu 24.04.4 LTS (glibc 2.39, systemd 255) | x86_64 | 通过 | 覆盖升级场景:旧 daemon 数据保留、planet 自动恢复、tap 重建;与官方 zerotier-one 共存 |
| 2026-11-01 | huaweimatebook | Windows 11 专业版 build 26200 | x86_64 | 通过(POC 级) | Windows POC 三道 Gate 全 PASS(实机 MSVC 编译/wintun 设备层/与 mac 节点互 ping 31/32);MSVC 14.44 + LLVM 18.1.8 + rustc 1.99.1;最低 Win10 未验证,详见 docs/2026-10-31-windows-poc-report.md |
| 2026-11-02 | huaweimatebook | Windows 11 专业版 build 26200 | x86_64 | 通过(安装级) | 正式版 e2e 全流程:离线 zip 安装→CLI↔daemon 互操→join/授权→与 mac 节点双向互 ping→stop/start→uninstall(数据保留)→重装状态自动恢复→实机重启开机自启;全程与官方 zerotier-one 共存无干扰;产物为 mac 交叉构建(cargo-xwin) |
| 2026-11-06 | openclaw (192.168.8.220) | Ubuntu 24.04.4 LTS (glibc 2.39, systemd 255) | x86_64 | 通过(安装级) | v0.3.0 正式产物覆盖升级 e2e(0.1.0→0.3.0):手动下载 linux_amd64 tar.gz + 包内 `planet-port install`→planet 3530253632 自动恢复(同地址 618857da15,PLANET 根 8ms)、网络 5cf54f181554090f 自动 re-join(tap0 / 10.121.16.71)、stop/start、幂等重装、开机自启 enabled;uninstall 未做(该机承载在跑网络)。线上安装链路本次未走通:install.sh 资产命名(`planet-port-<target>.tar.xz`)与 v0.3.0 实际产物(`planet-port_0.3.0_linux_amd64.tar.gz`)不符,且该机直连 github.com 不稳定;前者已于 2026-11-07 修复(install.sh/install.ps1 对旧命名资产 404 回退,引导脚本拉 main 分支即生效),后者属网络环境限制 |
| 2026-11-07 | huaweimatebook (192.168.8.190) | Windows 11 专业版 build 26200 | x86_64 | 通过(安装级) | v0.3.0 正式产物覆盖升级 e2e(0.2.0→0.3.0):zip 经 scp 传入(该机直连 github 被 reset),install-offline.ps1→旧服务停、三件套装至 Program Files、PATH 注册、服务即启;planet 3530253632 自动恢复(同地址 7d777b1295,15ms)、网络 5cf54f181554090f 自动 re-join(10.121.16.223);stop/start、幂等重装;与 openclaw(v0.3.0 linux_amd64)双向互 ping 0% 丢包(往返 180-370ms,补齐 11-06 未覆盖的跨节点项)。线上安装链路未走通:直连 github reset(环境限制)+ install.ps1 资产命名与实际产物不符(已于 2026-11-07 修复,install.ps1 旧命名 .zip 404 回退) |
| 2026-11-07 | Docker 容器(Apple Silicon, Docker Desktop 29.3) | Rocky Linux 9(glibc 2.34, systemd 250) | aarch64 | 通过(安装级,容器) | v0.3.0 linux_arm64 正式产物补测 rpm 系(glibc 2.34 恰为支持下限):全新 install→systemd 服务即启、add planet、planet enable/join 语义(v0.3.0:有网络才自动上线)、stop/start、幂等重装全通过,tap0 设备正常创建。UDP 数据面未能与 PLANET 根完成握手(root 一直 relay 无延迟),同 planet 宿主机正常,属 Docker Desktop NAT 环境限制;SELinux enforcing 未测(容器内为 disabled);跨节点互 ping 未覆盖 |
| 2026-11-07 | casaos (192.168.8.211) | Armbian 25.08(Debian 13 trixie, glibc 2.41, systemd 257) | aarch64 | 通过(安装级) | v0.3.0 linux_arm64 全新安装 e2e(此前实机仅测过覆盖升级,本机为无旧版本净装):install→systemd 服务即启+开机自启、add planet、join 私有网络(控制器授权后 OK,managed IP 10.121.16.99/24)、PLANET 根 UDP 直连 11-31ms、stop/start、幂等重装(planet 自动恢复,同地址 7e17427041);与 openclaw(amd64)/huaweimatebook(Windows)跨节点互 ping 均 0% 丢包(20ms/112ms) |
| 2026-11-07 | WX-RK3566 (10.121.15.88) | Ubuntu 20.04.6 LTS (glibc 2.31) | aarch64 | 通过(安装级) | v0.3.0 linux_arm64 覆盖升级 e2e(0.1.0→0.3.0):install、planet 3530253632 自动恢复(同地址 7b1de38f18)、网络 5cf54f181554090f OK(10.121.16.244)、stop/start、幂等重装、ping openclaw 0% 丢包;planet 文件重复导入 409 防覆盖符合预期;**glibc 2.31 实测可加载运行,实际兼容下限低于本文件「已知边界」写的 2.34** |
| 2026-11-07 | gozi-mba (10.152.124.24) | Linux Mint 21.3 (glibc 2.35) | x86_64 | 通过(安装级) | v0.3.0 linux_amd64 覆盖升级 e2e(0.1.0→0.3.0):install、planet 自动恢复(同地址 f5107745dd)、网络 OK(10.121.16.161)、stop/start、幂等重装、ping openclaw 0% 丢包;planet 文件重复导入 409 防覆盖符合预期;本轮由 scripts/e2e-host-test.sh 固化流程自动执行 |

## 已知边界(实测结论)

- Linux 支持范围:amd64 / arm64,glibc ≥ 2.34(systemd 发行版);SELinux enforcing(Fedora/RHEL 系)未测不在支持内。
- `macos_x86_64` 产物已构建,尚未在 Intel Mac 实机验证。
- Windows:安装级验证通过(Win11 实机,2026-11-02,离线与线上安装路径);实机现运行 v0.2.0-beta.2(线上安装升级,beta.1→beta.2 覆盖升级后网络自动恢复)。线上安装实测(2026-11-02,仓库已公开):github.com 资产下载与 api.github.com 匿名可达,安装链路全通;raw.githubusercontent.com 引导下载在家宽出口被阻断(与仓库公开无关),此时可改为浏览器直下 release zip 走离线安装;`/releases/latest` 尚无正式版返回 404 属语义正确(该时点记录);latest 现已指向 v0.3.0 旧命名产物,2026-11-07 起 install.ps1/install.sh 对其 404 回退后 latest 直装可用。NRPT 规则生命周期 2026-11-02 全链路实测通过:控制器下发 DNS 配置(域+服务器)且客户端 `dnsMode=network` 时建规则(Comment 带 `planet-port:<nwid>` 可追溯)、ConfigUpdate 多轮幂等、leave 删规则、重 join 后重设 dnsMode 规则重建、daemon 被强杀后 NRPT 残留由服务自启时按前缀全清兜底。**分流解析在本实机不生效**:系统解析器不消费 NRPT(手工 PowerShell 建的规则同样被无视,适配器直设 DNS 也不发查询),实机 build 26100.9711 落在 24H2 已知 VPN/NRPT 回归区间且装有腾讯电脑管家,属环境限制而非产品缺陷;如遇此情况建议换机复测或升级系统补丁。适配器自愈:2026-11-02 起驱动线程 10s 巡检,适配器被外部重启(Restart-NetAdapter)后约 25-40s 自动重建设备恢复通信(实机验证)。纸面支持 Win10+(按支持面收敛),未实测,结论按 Win11 记录。已知平台边界:设备面 IPv6 帧静默丢弃、IPv6 地址/托管路由不安装(L3 + ARP 垫层);Windows 服务日志不落盘(`service logs` 不支持);不做代码签名,SmartScreen 拦截属预期;进程/服务/系统重启后网络恢复有约 1-3 分钟等待期(ARP 解析与路径重建,期间 ping 超时属预期);Windows 会话经 cmd 调 CLI 时引号被双层解析会报"找不到网络路径"(PowerShell 单引号调用无此问题)。
- 与官方 zerotier-one 共存:五台均验证互不干扰(接口、端口独立;Windows 侧为官方 app 与本产品双装全程并行)。

e2e 验证脚本步骤:install → status → info → journalctl 日志 → stop/start → 开机自启 → 幂等重装 → uninstall(unit 删除、数据保留)→ 重装留运行。
