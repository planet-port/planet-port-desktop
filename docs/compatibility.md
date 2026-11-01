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

## 已知边界(实测结论)

- Linux 支持范围:amd64 / arm64,glibc ≥ 2.31(systemd 发行版);SELinux enforcing(Fedora/RHEL 系)未测不在支持内。
- `macos_x86_64` 产物已构建,尚未在 Intel Mac 实机验证。
- Windows:POC 级验证通过(Win11 实机,三道 Gate);**正式版已实现**(wintun 数据面、SCM 服务化、CLI 全命令、DNS NRPT、install.ps1 / 离线 zip 安装链路),**待 Win11 实机安装级 e2e 复验**——上表 Win11 行仍记「通过(POC 级)」,安装级实测后回填;纸面支持 Win10+(按支持面收敛),未实测,结论按 Win11 记录。已知平台边界:设备面 IPv6 帧静默丢弃、IPv6 地址/托管路由不安装(L3 + ARP 垫层);Windows 服务日志不落盘(`service logs` 不支持);不做代码签名,SmartScreen 拦截属预期。
- 与官方 zerotier-one 共存:四台均验证互不干扰(接口、端口独立)。

e2e 验证脚本步骤:install → status → info → journalctl 日志 → stop/start → 开机自启 → 幂等重装 → uninstall(unit 删除、数据保留)→ 重装留运行。
