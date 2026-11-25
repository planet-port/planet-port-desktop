export interface TransportStatus {
  primaryPort: number;
  secondaryPort: number | null;
  tcp: string; // disabled | idle | connecting | established
  relay: string | null;
}

export interface PlanetStatus {
  id: string;
  name: string;
  icon: string;
  color: string;
  type: 'official' | 'custom';
  status: string; // running | stopped
  enabled: boolean;
  online: boolean;
  nodeId: string | null;
  coreVersion: string | null;
  latencyMs: number | null;
  networkCount: number;
  port: number;
  transport: TransportStatus | null;
  tcpFallbackRelay: string | null;
}

/// GET /planets/{id} 响应(与 daemon PlanetDetail camelCase 序列化同构)
export interface PlanetDetail {
  id: string;
  name: string;
  icon: string;
  color: string;
  type: 'official' | 'custom';
  worldId: number;
  roots: string[];
  fileSize: number;
  revisionTimeMs: number;
  lastNodeId: string | null;
  source: string;
  networkCount: number;
  port: number;
  status: string;
  enabled: boolean;
  online: boolean;
  transport: TransportStatus | null;
  tcpFallbackRelay: string | null;
}

export interface RouteLine {
  target: string;
  via: string | null;
}

export interface NetworkEntry {
  planet: string;
  nwid: string;
  name: string | null;
  /// 核心/daemon 合成态:OK | REQUESTING_CONFIGURATION | ACCESS_DENIED | …
  /// | STOPPED | DISABLED | DEVICE_ERROR(核心 OK 但本地设备层失败,数据面不通)
  status: string;
  conflict: string | null; // address | subnet | timeout
  type: string; // PUBLIC | PRIVATE
  mac: string;
  interface: string | null;
  mtu: number;
  assignedAddrs: string[];
  managedRoutes: RouteLine[];
  dnsServers: string[];
  routeViaZeroTier: boolean;
  routeViaLocked: boolean;
  dnsMode: string; // none | network | custom
  customDns: string[];
}

export interface PeerEntry {
  planet: string;
  address: string;
  role: string; // PLANET | MOON | LEAF
  version: string | null;
  latencyMs: number | null;
  path: string | null; // null = relay
}

/// 与 Rust GuiError(serde tag=kind) 对齐
export interface GuiError {
  kind: 'offline' | 'api' | 'usage';
  status?: number;
  code?: string | null;
  message: string;
  candidates?: string[] | null;
}

export interface DaemonState {
  online: boolean;
  installed: boolean;
  /// 已装/运行中 daemon 版本(在线 /version、离线 bin --version);不可得为 null
  daemonVersion: string | null;
}

/// bundle 内嵌安装资源清单(embedded_manifest 返回;dev/未组包时为 null)
export interface EmbeddedManifest {
  version: string;
  target: string;
}

/// GET /identity 响应(与 daemon api_identity.rs 响应同构)
export interface IdentityInfo {
  global: { exists: boolean; nodeId: string | null };
  mode: string; // shared | independent(PUT /identity 可切换;行星运行中 409 拒绝)
  planets: Array<{ id: string; name: string; nodeId: string | null; running: boolean }>;
}
