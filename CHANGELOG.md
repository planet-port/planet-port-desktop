# 更新日志

所有显著变更均记录于此。版本号遵循[语义化版本](https://semver.org/lang/zh-CN/)。

## v0.4.0（2026-11-12）
### 修复

- **ci：**补回 release.sh 可执行位
- **zt-core：**Windows 副口换绑句柄身份错位与两处告警语义
- **zt-core：**TCP 隧道写超时与控制路径非阻塞防节点冻结
- **daemon：**PATCH 校验前置防部分应用并收紧 relay 类型检查
- **daemon：**迁移补 dataDir 祖先/嵌套守卫防递归与误拖
- **diag：**PlanetDetail 补 transport 与 relay 配置态回显
- **cli：**info 表显示节点地址与版本,列结构对齐官方口径
- **core：**终审必修项——迁移删源失败不上抛与 clippy 闭包清理
### 文档

- tcp-proxy 部署指引
### 新增

- **ci：**新增网页发版工作流
- **release：**接入 git-cliff 自动发版并统一网页发版
- **scripts：**新增 zerotier-one 官方服务启停脚本
- **zt-core：**通道层纯逻辑(伪TLS线协议/选路决策/副口绑定)
- **zt-core：**副端口通道——离线换绑撞活流对齐官方
- **zt-core：**TCP 隧道兜底通道(伪TLS线协议)
- **daemon：**per-planet TCP relay 配置与 9993 优先端口分配
- **daemon：**registry 遗留条目启动迁移与 relay 装配
- **diag：**通道状态可见性(dump/info/transport 事件)
- **cli：**info 表末尾增加 NETWORKS 网络计数列
- **build：**tcp-proxy 默认监听口改为 9443 并支持自定义
- **release：**开发版本占位 -dev 号,与已发布正式版永不同号
- **cli：**set-relay 支持精简形,复用 planet 根端点展开完整地址
- **cli：**info 表增 RELAY 列显示 TCP 兜底 relay 配置态
### 构建

- **scripts：**tcp-proxy 编译脚本(macOS 本地 + zig 交叉 Linux)
## v0.3.5（2026-11-07）
### 修复

- **install：**版本差异确认按版本号判定升/降级
### 新增

- **install：**安装脚本支持 --proxy 代理前缀并强制 sha256 校验
## v0.3.4（2026-11-07）
### 修复

- **e2e：**产物缓存命中时同架构第二台解析不到二进制目录
- **identity：**补 Windows 分支 tighten 闭包返回类型标注
### 新增

- **identity：**导入官方身份后自动切换共享模式并指引游离记录清理
### 重构

- **identity：**导入模式与相同 planet 身份选择交还用户
## v0.3.3（2026-11-07）
### 修复

- **scripts：**e2e 产物下载适配 v0.3.1 起 dash 命名 tar.xz 资产
- **e2e：**资产解包兼容 v0.3.1+ 嵌套目录布局
- **ci：**manylinux 容器内补装 libclang 修复 bindgen 构建
### 构建

- Linux 产物改在 manylinux_2_28 容器构建,glibc 基线降至 2.28
## v0.3.2（2026-11-06）
### 修复

- **zt-core：**托管路由不压本机直连路由 + CLI 管道静默退出
### 文档

- 补记 v0.3.0 多机型部署实测并同步 RHEL 系支持状态
### 新增

- **scripts：**固化主机级部署 e2e 测试脚本
## v0.3.1（2026-11-06）
### 修复

- **install：**glibc 基线放宽至 2.34 并豁免沙箱模式提权
- **install：**latest 旧命名资产回退修复一条命令安装 404
- **install：**回退旧命名资产按实际格式解包
### 文档

- **compat：**补记 huaweimatebook v0.3.0 覆盖升级 e2e 实测记录
## v0.3.1-beta.2（2026-11-06）
### 修复

- **build：**补 allow-dirty 跳过 CI 再生成比对并清理残留引用
- **install：**修复 GNU find 兼容并同步 Windows 资产命名
- **build：**补 dist profile 并消除构建注入文件的 workflow 噪音
- **ci：**构建前拉取 ZeroTier 核心源码并兼容 Linux SHA256 工具
### 文档

- README 平台状态同步至 Linux / Windows CLI 已发布
- **compat：**补记 openclaw v0.3.0 覆盖升级 e2e 实测记录
### 新增

- **install：**install.sh 适配 cargo-dist 产物命名
### 构建

- 接入 cargo-dist 发布流水线并将 daemon 并入 cli 单包双 bin
## v0.3.0（2026-11-06）
### 修复

- **daemon：**修正 Windows 巡检判活的两个字段陷阱
- **daemon：**身份切换补 runtime_gate 并校验 secret 内容
- **daemon：**修正 service_control 跨平台 unused import
- **daemon：**修正身份导入校验下标与补运行态守卫、双文件原子写
- **cli：**identity 导入改走长超时路径并类型化列表输出
- **poc4：**StateStore 副本对齐 zt-core 四参签名修复 Windows 编译
- **daemon：**身份导入持 runtime_gate 收窄启动竞态并前置权限收紧
- **daemon：**身份导入补双文件回滚与 gate 内覆盖重判
- **cli：**identity use 成功行恢复本地化并修正帮助文案
### 文档

- 补线上安装实测结论与 raw 引导阻断边界
- 身份管理层设计(全局/独立身份与官方身份迁移)
- 身份管理层实施计划
- NRPT 全链路实测销项与适配器自愈记录
- wintun 适配器自愈实施计划
- 安装自动注册 PATH 与换行符约定设计
- README 补 identity 命令说明
- 身份模式全局化说明
- 身份隔离注释限定与切换文案去嵌套括号
- 精简 README 特色描述
### 新增

- **daemon：**Windows 适配器失效自愈重建
- **cli：**Windows 安装自动注册 Machine PATH 并约定换行符
- **zt-core：**NodeParams 支持注入全局身份目录
- **daemon：**planet 注册表新增身份绑定字段
- **daemon：**新增官方 ZeroTier 服务停用模块
- **daemon：**身份查询/导入/切换绑定 REST 端点
- **cli：**新增 identity 命令
- **identity：**官方 planet 识别与导入确认提示
- **cli：**leave 无 --planet 时按 nwid 跨行星唯一定位
- **daemon：**planet 启用禁用开关与派生运行规则
- **daemon,cli：**启停语义并入 enable/disable 并重构列表分组输出
### 样式

- **cli：**改写 symlink 清理分支消除 clippy single-match
### 重构

- **daemon：**身份模式改为 daemon 级全局开关
- **cli：**identity use 改为全局模式开关
## v0.2.0-beta.2（2026-11-01）
### 文档

- 兼容性记录更新线上安装验证边界
## v0.2.0-beta.1（2026-11-01）
### 修复

- **poc4：**终审收尾小修(gate1 空包守卫与文档勘误)
- **daemon：**服务线程 panic 时以服务特定错误上报停止
- **cli：**sc query 状态解析补数字兜底
- **cli：**修正服务状态 raw 值判定(STOPPED=1)
- **cli：**Windows 卸载的 NRPT 清理改按 Comment 前缀匹配
- **daemon：**托管域撤销时清理残留 NRPT 规则
- **scripts：**安装脚本强制 TLS1.2 并补参数校验与缺件汇总
- **cli：**顶层 install 命令支持 Windows 二进制定位
- **api：**Windows 客户端解析系统数据目录并对齐 token 可读性
- **cli：**服务失败自拉起覆盖后续全部失败
### 文档

- 新增 Windows 支持评估 POC 设计
- Windows 支持评估 POC 实施计划
- 实施计划同步 poc4 独立 crate 结构
- **poc4：**Windows 支持评估阶段报告与兼容性记录
- Windows POC 三道 Gate 实机实测结果回填
- wincli 正式 Windows 客户端设计
- wincli 实施计划
- Windows 安装与命令文档
- 修正 Windows 文档与安装脚本的归因表述
### 新增

- **poc4：**poc4 Windows 设备层骨架(wintun L3)
- **poc4：**poc4 Windows 地址路由应用与收包自验
- **poc4：**poc4 Windows 版 ZeroTier node 驱动(控制面)
- **poc4：**poc4 Windows 数据面(wintun 帧桥 + IPv4 ARP 垫层)
- **zt-core：**node 驱动 Windows 分叉(UDP 读线程+mpsc 唤醒)
- **zt-core：**设备层 Windows 化(wintun L3+ARP 垫层)
- **zt-core：**managed 路由 Windows 后端
- **daemon：**Windows 平台适配(信号/单例锁)
- **daemon：**Windows Service SCM 入口
- **cli：**Windows 服务管理(sc.exe 后端)
- **daemon：**DNS Windows 接线(NRPT)
### 构建

- **zt-sys：**Windows 目标交叉编译兼容修正
- **zt-sys：**修复 Windows 目标静态链接
- Windows 产物打包与安装脚本
## v0.1.2-beta.1（2026-10-31）
### 修复

- **cli：**修复多视角评估报告的 17 项反馈问题
- **cli：**service install 在 bootout 后等待旧服务真正退出再 bootstrap
- **cli：**交互选择器在死 TTY 下 CPU 自旋且无法被 SIGTERM 终止
- **daemon：**leave 同时清除 net_local 记录,不再被重启复活
- **scripts：**修正 Linux 目标产物目录后缀剥离残留 .2
- **cli：**修复 Linux 状态诊断与帮助文案的平台表述
### 文档

- 扩充 CLI 规范并新增手工回归清单
- **cli：**四视角 CLI 功能校验报告
- 整理 CLI 全命令标准输出规范
- 安装指南收敛为在线/离线两条路径,.pkg 延后评估
- Linux systemd 安装流程实施计划
- 安装指南与命令规范补 Linux 平台说明
- 新增平台兼容性实测记录
### 新增

- **scripts：**新增发版打包脚本
- **linux：**zt-core 与配置目录支持 Linux 编译与运行
- **daemon：**服务停止保留行星状态并支持恢复,list 全行星展示
- **cli：**多语言支持,简中/繁中/英语跟随系统语言
- **mockd：**监听端口支持 MOCKD_PORT 环境变量覆盖
- **daemon：**join/leave 幂等,重复操作静默成功
- **cli：**list/listnetworks 多 managed IP 全量堆叠展示
- **cli：**补齐全命令 -j JSON 输出,机器可读无死角
- **api,cli：**网络列表与详情展示 MAC 与接口名
- **api,cli,daemon：**planet 与网络定位支持别名且不区分大小写
- **cli,daemon：**单行星自动选择按登记数计,不看连接状态
- **cli：**新增 install 一键自安装命令,统一在线/离线安装路径
- **cli：**install 版本差异确认,支持快速升降级
- **cli：**service 新增 Linux systemd 平台实现
- **scripts：**install.sh 支持 Linux amd64/arm64 在线安装
- **scripts：**release.sh 增加 linux_amd64/linux_arm64 产物
- **scripts：**release.sh 支持 beta 标签发布
### 重构

- 移除 mockd 契约 mock daemon
- **cli：**移除交互式 planet 选择器
- **cli：**service 拆分平台模块,launchd 实现移入 macos.rs
- **cli：**install 落盘路径收敛到平台模块
## v0.1.1（2026-10-29）
### 修复

- **cli：**service start/stop 等待以 pid 为准,daemon.log 迁移并接入轮转
- **daemon：**feth 清扫销毁失败时保留登记供下次重试
- **cli：**选择器 raw 模式补信号防护
### 新增

- **cli：**列表输出 planet id 后追加名称
- **scripts：**新增安装/卸载脚本
## v0.1.0（2026-10-29）
### 修复

- **mockd：**统一 planet 引用为 canonical id 并支持名称过滤
- **api：**契约响应字段改为 camelCase
- **mockd：**/planets 形状对齐 PlanetStatus 并补 stopped 拒变更与事件归属
- **cli：**set 校验 routeViaZeroTier 取值并拒绝空参数列表
- **cli：**HTTP 请求加 10s 全局超时并保证 planet rm --json 输出纯净
- **mockd：**查询过滤的 planet 解析不再要求 running
- **build：**bootstrap 缓存损坏分支先删缓存再退出
- **core：**路由记账仅记成功写入并补充设备入向帧计数
- **pocs：**poc3 断言 4 计数与断言 5 判定语义修正
- **license：**补齐 Core/ 下 MPL-2.0 全文分发
- **daemon：**修复空 token 文件导致的静默轮换与空配置目录环境变量泄漏
- **daemon：**收紧 dataDir 删除守卫并按 metadata 预检源文件大小
- **daemon：**修正 stop 红线顺序并收敛启停与 delete 的竞态
- **daemon,cli：**修正 RunAtLoad 语义与信号注册失败的静默退出
- **daemon：**消除信号单侧注册失败时 select! 的 panic 路径
- **daemon：**修复终审发现的重启恢复与事件语义问题
- **daemon：**dnsMode 切回 none 时立即还原系统 DNS
- **daemon：**DNS 托管路径串行化并修正标记与单实例守卫的错误报告
- **daemon：**netconf 的 DNS 调用移入阻塞线程池并修正 errno 注释
- **daemon,cli：**planet 导入后自动上线并如实报告状态
- **daemon：**修正 DNS 服务解析并对原手动 DNS 做快照还原
- **daemon：**崩溃残留的 feth 孤儿设备启动时清扫
- **cli：**service install 误把 CLI 自身拷贝为 daemon 二进制
- **daemon：**feth 登记任务改为落盘前重读最新设备快照
### 文档

- README 补充组件说明与命令参考
- README 补充环境变量说明与 --follow、rm --json 行为
- daemon 一期 POC(1-3)设计 spec
- 核心库依赖管理改为 bootstrap 模式
- spec 验证方式措辞修正
- 并入 POC 前置调查的三项事实修正
- daemon 一期 POC(1-3)实现计划
- **poc：**spec §8 收口 POC 3 最终结果与环境问题定案
- **daemon：**planet-portd 一期设计 spec
- **daemon：**一期实现计划
- **spec：**对齐复审反馈的三处文档偏差
### 新增

- **api：**定义 REST 契约类型与端点构造函数
- **mockd：**实现契约 mock daemon
- **cli：**CLI 骨架、HTTP 客户端与错误退出码映射
- **cli：**查询类命令与聚合表格输出
- **cli：**变更类命令与 planet 定位链客户端侧实现
- **cli：**planet 子命令组与 emoji 图标渲染
- **cli：**use 与 events 命令
- **core：**zt-core 封装层与状态存储
- **core：**驱动线程与 UDP 通路,POC1 控制面打通
- **core：**feth L2 设备层与 managed 配置应用,POC2 数据面
- **pocs：**双 Node 同进程隔离验证 bin 与 POC 结果记录
- **core：**peers/orbit 封装与驱动线程加固清单落地
- **daemon：**planet-portd 骨架
- **daemon：**planet 导入与生命周期管理端点
- **daemon：**实例运行时与网络面(start/stop、join/leave/set、networks、events)
- **daemon：**peers/moons/dump 端点(moon 落盘与 orbit/deorbit)
- **daemon：**SIGINT/SIGTERM 信号优雅退出
- **cli：**service 组命令与端口文件协商
- **cli：**planet 生命周期命令提级为顶层 add/remove/start/stop
- **cli,daemon：**交互式 planet 选择、分组 list 与 remove 自动清理
- **cli：**events 必须指定 planet
- **daemon：**加单实例守卫与 Fatal 自动收尾
- **daemon：**netconf 应用阶段写系统 DNS 并在停止时还原
- **cli,daemon：**LaunchDaemon 崩溃自动拉起与 planet URL 导入
- **cli：**多选器空选回车默认选中高亮项
- **cli：**表格输出改为传统空格对齐列
- **cli：**交互式选择器改为 --ui 显式启用,默认非交互
- **cli：**service start/stop 等待实际结果,服务标识对齐命名规则
### 构建

- bootstrap 拉取核心源码并产出 zt-sys 绑定
### 样式

- **zt-core：**device-reader 停止日志带上网络 id
- **daemon：**优雅退出日志改为动作式中文描述
### 重构

- **cli：**events 定位与 dump 完全对齐

