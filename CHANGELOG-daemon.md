# daemon 更新日志

版本号遵循[语义化版本](https://semver.org/lang/zh-CN/)。仅记录 daemon 侧用户可感知变更。

## planet-portd/v0.6.0（2026-11-28）
### 修复

- **zt-core：**网段冲突自动断开后误判 rc=1000 致 planet 整体停机
### 新增

- **daemon：**网络级启用/禁用概念移除,改为连接/断开
- **daemon：**join/leave 语义对齐用户层,remove 独立彻底删除

## planet-portd/v0.5.1（2026-11-26）
### 修复

- **daemon：**--version 与 /version 构建时间统一为本地时间无时区后缀

## planet-portd/v0.5.1-beta.1（2026-11-25）
### 修复

- **daemon：**--version 与 /version 构建时间统一为本地时间无时区后缀

