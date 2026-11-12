# tcp-proxy 部署指引(TCP 兜底 relay)

## 它是什么、为什么需要

运营商/家宽/高铁等网络对 ZeroTier 根(UDP 9993)的新建会话丢包时,
planet-port 的副端口探测通常能撞出活流恢复连接;当 UDP 完全不可用
时,最后防线是 TCP 隧道——但 **zerotier-one 根服务器的 TCP 9993 是
本地 HTTP 控制面,不接收 ZT-over-TCP**。官方的 TCP 兜底发给独立的
`tcp-proxy` 组件:一条 TCP 连接上跑伪 TLS 记录层封装的 ZT 包,
proxy 在服务器本地代为 UDP 转发到根。

```
planet-port ──TCP 9443──▶ tcp-proxy ──UDP 127.0.0.1:9993──▶ zerotier-one(根)
```

组件源码为官方 ZeroTier 仓库 `tcp-proxy/`(Apache-2.0)。官方源码监听口
宏写死 443 且二进制无端口参数,本仓库编译脚本默认改为 **9443**(避开
443 常被 Web 服务占用),可用环境变量覆盖,见下。

## 编译

```bash
./scripts/build-tcp-proxy.sh x86_64-linux-gnu   # 按服务器 uname -m 选
# 服务器是 aarch64 则: ./scripts/build-tcp-proxy.sh aarch64-linux-gnu
# 自定义监听口: TCP_PROXY_PORT=8443 ./scripts/build-tcp-proxy.sh x86_64-linux-gnu
```

产物 `dist-tcp-proxy/tcp-proxy-x86_64-linux-gnu`(默认监听 9443;自定义口
产物名带 `-p<port>` 后缀,如 `tcp-proxy-x86_64-linux-gnu-p8443`)。先
`ssh root@<服务器> uname -m` 确认架构(x86_64 → x86_64-linux-gnu;aarch64
→ aarch64-linux-gnu)。

Apache-2.0 要求:分发二进制须附源码获取方式(官方仓库
https://github.com/zerotier/ZeroTierOne )与 Apache-2.0 许可文本;
归档产物时本文档与许可文本一并存放。

## 部署(与根同机;以 gozi25 为例)

```bash
scp dist-tcp-proxy/tcp-proxy-x86_64-linux-gnu root@124.222.91.224:/usr/local/bin/tcp-proxy
ssh root@124.222.91.224 chmod +x /usr/local/bin/tcp-proxy
```

systemd 单元 `/etc/systemd/system/zt-tcp-proxy.service`(官方仓库
tcp-proxy/zerotier-proxy.service 同款语义):

```ini
[Unit]
Description=ZeroTier TCP fallback proxy
After=network.target

[Service]
ExecStart=/usr/local/bin/zt-tcp-proxy
Restart=always
RestartSec=5
# 监听 TCP 9443(本仓库编译产物默认;改端口须同步客户端 relay 配置)

[Install]
WantedBy=multi-user.target
```

```bash
systemctl daemon-reload && systemctl enable --now zt-tcp-proxy
ss -tlnp | grep 9443   # 确认监听
```

防火墙/安全组放行 TCP 9443 入站(腾讯云 lighthouse 控制台安全组)。

## 客户端配置(planet-port)

```bash
# 三种写法等价(relay 与根同机时推荐精简形,端点随 planet 自动对齐):
planet-port planet set-relay gozi25 --relay 124.222.91.224:9443   # 完整
planet-port planet set-relay gozi25 --relay :9443                 # 复用根端点 ip
planet-port planet set-relay gozi25 --relay                       # 根端点 ip + 默认 9443
# 纯端口号 --relay 9443 与 :9443 同义;不带 --relay 即清除
planet-port dump --planet gozi25   # 看 transport: ... tcp=established(...)
planet-port planet set-relay gozi25
```

relay 只是兜底:UDP 正常时隧道不激活(`tcp=idle`),60 秒无 global UDP
直收才建立,UDP 恢复后 30 秒内主动关闭(fail forward 回 UDP)。

## 验证

1. 正常网络:`dump` 中 `tcp=idle`(UDP 正常,隧道不激活)
2. 断 UDP 验证(本机 root):
   ```bash
   # 先 dump 核对主口实际端口(下例 9993;主口可自定义,规则须与之匹配)
   echo "block drop out quick proto udp from any port 9993 to 124.222.91.224" \
     | sudo pfctl -ef -
   # 约 60-90 秒后 dump 应见 tcp=established,peers 根路径恢复
   # (注:-ef - 测试窗口内会临时替换系统运行 ruleset,测完立即恢复)
   sudo pfctl -f /etc/pf.conf && sudo pfctl -d
   # 恢复必须 -f 重载原 ruleset 再 -d:仅 -d 会残留 block 规则,
   # 日后 pf 再启用时静默阻断 UDP→根,极难排查
   ```
3. 服务器侧:`journalctl -u tcp-proxy -f` 可见客户端连接与转发日志

## 已知边界

- proxy 对目的端口 ≤1024 的包拒转(官方滥用防护);ZeroTier 根在 9993,不受影响
- proxy 每客户端占一个本地 UDP 源口(1024 起),连接空闲 300s 服务端踢线
  (客户端会在下次触发条件满足时自动重连)
- relay 与根同机部署时,服务器整机不可达则 TCP 兜底同样失效(单点);
  可另行部署近端多 relay,客户端配最快者
