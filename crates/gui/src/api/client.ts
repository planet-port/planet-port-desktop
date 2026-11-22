import { invoke } from '@tauri-apps/api/core';
import type { DaemonState, EmbeddedManifest, GuiError, IdentityInfo, NetworkEntry, PeerEntry, PlanetDetail, PlanetStatus } from './types';

/// invoke 失败时 reject 值即 Rust 侧序列化的 GuiError 结构
export function asError(e: unknown): GuiError {
  return e as GuiError;
}

async function cmd<T>(name: string, args?: Record<string, unknown>): Promise<T> {
  return invoke<T>(name, args);
}

export const api = {
  daemonState: () => cmd<DaemonState>('get_daemon_state'),
  /// 内嵌安装资源清单:null = 无内嵌资源,前端退回手动引导
  embeddedManifest: () => cmd<EmbeddedManifest | null>('embedded_manifest'),
  /// 启动提权安装:Ok 仅代表流程已启动,结果走 install-done/failed 事件;
  /// 提权前失败(缺资源/拷贝 staging)走 rejection,前端须 catch 转 failed 态
  startInstall: () => cmd<void>('start_install'),
  status: () => cmd<{ planets: PlanetStatus[] }>('get_status'),
  planets: () => cmd<{ planets: PlanetStatus[] }>('get_planets'),
  addPlanet: (source: string, name?: string, icon?: string, color?: string) =>
    cmd('add_planet', { source, name: name ?? null, icon: icon ?? null, color: color ?? null }),
  removePlanet: (id: string) => cmd('remove_planet', { id }),
  enablePlanet: (id: string) => cmd('enable_planet', { id }),
  disablePlanet: (id: string) => cmd('disable_planet', { id }),
  networks: (planet?: string) => cmd<{ networks: NetworkEntry[] }>('get_networks', { planet: planet ?? null }),
  joinNetwork: (nwid: string, planet?: string) => cmd('join_network', { nwid, planet: planet ?? null }),
  leaveNetwork: (nwid: string, planet?: string) => cmd('leave_network', { nwid, planet: planet ?? null }),
  enableNetwork: (nwid: string, planet?: string) => cmd('enable_network', { nwid, planet: planet ?? null }),
  disableNetwork: (nwid: string, planet?: string) => cmd('disable_network', { nwid, planet: planet ?? null }),
  setNetwork: (
    nwid: string,
    body: { routeViaZeroTier?: boolean; dnsMode?: string; customDns?: string[] },
    planet?: string,
  ) => cmd('set_network', { nwid, body, planet: planet ?? null }),
  peers: (planet?: string) => cmd<{ peers: PeerEntry[] }>('get_peers', { planet: planet ?? null }),
  planetExport: (id: string) => cmd<number[]>('planet_export', { id }),
  /// daemon GET /planets/{id} 返回信封 { planet: <detail> },此处解包
  planetDetail: (id: string) =>
    cmd<{ planet: PlanetDetail }>('planet_detail', { id }).then(r => r.planet),
  planetUpdate: (id: string, name: string, icon: string, color: string) =>
    cmd('planet_update', { id, name, icon, color }),
  identityGet: () => cmd<IdentityInfo>('identity_get'),
  /// 身份模式切换(PUT /identity):行星运行中 daemon 409 拒绝,前端预检禁用为主
  identityModeSet: (mode: 'shared' | 'independent') => cmd('identity_mode_set', { mode }),
  identityPreflight: (path: string) => cmd<Record<string, unknown>>('identity_import_preflight', { path }),
  identityImport: (path: string, force: boolean) => cmd<Record<string, unknown>>('identity_import', { path, force }),
  writeBytes: (path: string, bytes: number[] | Uint8Array) =>
    cmd<void>('write_file_bytes', { path, bytes: Array.from(bytes) }),
};
