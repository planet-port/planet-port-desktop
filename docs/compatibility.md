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

## 已知边界(实测结论)

- Linux 支持范围:amd64 / arm64,glibc ≥ 2.31(systemd 发行版);SELinux enforcing(Fedora/RHEL 系)未测不在支持内。
- `macos_x86_64` 产物已构建,尚未在 Intel Mac 实机验证。
- Windows:安装级验证通过(Win11 实机,2026-11-02,离线安装路径);实机现运行 v0.2.0-beta.1 release 正式产物。线上 iex 安装未完整实测:下载段依赖仓库/资产匿名可达,当前仓库私有(raw/api/资产匿名 404,与 mac install.sh 同等限制),网络连通性已确认(404 为正常 HTTP 响应);安装段逻辑与离线版共用已实测。NRPT 建规则路径未实测(测试网络未下发 DNS 配置,`dnsMode=network` 下空 servers 正确不建规则,NRPT 清理幂等已实测);纸面支持 Win10+(按支持面收敛),未实测,结论按 Win11 记录。已知平台边界:设备面 IPv6 帧静默丢弃、IPv6 地址/托管路由不安装(L3 + ARP 垫层);Windows 服务日志不落盘(`service logs` 不支持);不做代码签名,SmartScreen 拦截属预期;进程/服务/系统重启后网络恢复有约 1-3 分钟等待期(ARP 解析与路径重建,期间 ping 超时属预期)。
- 与官方 zerotier-one 共存:五台均验证互不干扰(接口、端口独立;Windows 侧为官方 app 与本产品双装全程并行)。

e2e 验证脚本步骤:install → status → info → journalctl 日志 → stop/start → 开机自启 → 幂等重装 → uninstall(unit 删除、数据保留)→ 重装留运行。
