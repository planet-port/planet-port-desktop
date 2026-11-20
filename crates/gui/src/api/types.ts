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

export interface RouteLine {
  target: string;
  via: string | null;
}

export interface NetworkEntry {
  planet: string;
  nwid: string;
  name: string | null;
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
  code?: string;
  message: string;
  candidates?: string[] | null;
}

export interface DaemonState {
  online: boolean;
  installed: boolean;
}
