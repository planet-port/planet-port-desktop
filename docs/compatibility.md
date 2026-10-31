# 平台兼容性实测记录

每在一台新机型的实机上完成安装验证后,在下方表格补一行。发布矩阵见 docs/install.md ,
此表只记**实测过的机器**——发布产物支持但未实测的平台不算通过。

| 测试日期 | 机型 / 主机 | 系统 | 架构 | 验证结果 | 备注 |
|---|---|---|---|---|---|
| 2026-10-31 | MacBook Pro 16" (MacBookPro18,4) | macOS 26.6.2 | arm64 | 通过 | 开发机,daemon 常驻;launchd 路径 |
| 2026-10-31 | gozi-mba (10.152.124.24) | Linux Mint 21.3 (glibc 2.35) | x86_64 | 通过 | systemd 全流程 e2e;与官方 zerotier-one 共存 |
| 2026-10-31 | WX-RK3566 (10.121.15.88) | Ubuntu 20.04.6 LTS (glibc 2.31) | aarch64 | 通过 | systemd 全流程 e2e;与官方 zerotier-one 共存;glibc 基线机型 |
| 2026-10-31 | openclaw (192.168.8.220) | Ubuntu 24.04.4 LTS (glibc 2.39, systemd 255) | x86_64 | 通过 | 覆盖升级场景:旧 daemon 数据保留、planet 自动恢复、tap 重建;与官方 zerotier-one 共存 |

## 已知边界(实测结论)

- Linux 支持范围:amd64 / arm64,glibc ≥ 2.31(systemd 发行版);SELinux enforcing(Fedora/RHEL 系)未测不在支持内。
- `macos_x86_64` 产物已构建,尚未在 Intel Mac 实机验证。
- 与官方 zerotier-one 共存:四台均验证互不干扰(接口、端口独立)。

e2e 验证脚本步骤:install → status → info → journalctl 日志 → stop/start → 开机自启 → 幂等重装 → uninstall(unit 删除、数据保留)→ 重装留运行。
