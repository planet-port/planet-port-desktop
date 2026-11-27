# cli 更新日志

版本号遵循[语义化版本](https://semver.org/lang/zh-CN/)。仅记录 cli 侧用户可感知变更。

## planet-port/v0.7.0（2026-11-28）
### 修复

- **cli：**显式版本安装旧 tag 识别回归
- **zt-core：**网段冲突自动断开后误判 rc=1000 致 planet 整体停机
### 新增

- **cli：**命令面收敛为 join/leave/remove
- **cli：**listnetworks 冲突网络在 IP 列显示提示文字

## planet-port/v0.6.0（2026-11-26）
### 修复

- **cli：**--version 构建时间统一为本地时间无时区后缀
- **cli：**install.sh 显式版本参数适配组件化 tag
### 新增

- **cli：**--version 分行列出 cli 与 daemon 版本

## planet-port/v0.6.0-beta.1（2026-11-26）
### 修复

- **cli：**--version 构建时间统一为本地时间无时区后缀
- **cli：**install.sh 显式版本参数适配组件化 tag
### 新增

- **cli：**--version 分行列出 cli 与 daemon 版本

