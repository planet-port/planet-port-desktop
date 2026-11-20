import { invoke } from '@tauri-apps/api/core';
import type { DaemonState, GuiError, NetworkEntry, PeerEntry, PlanetStatus } from './types';

/// invoke 失败时 reject 值即 Rust 侧序列化的 GuiError 结构
export function asError(e: unknown): GuiError {
  return e as GuiError;
}

async function cmd<T>(name: string, args?: Record<string, unknown>): Promise<T> {
  return invoke<T>(name, args);
}

export const api = {
  daemonState: () => cmd<DaemonState>('get_daemon_state'),
  status: () => cmd<{ planets: PlanetStatus[] }>('get_status'),
  planets: () => cmd<{ planets: PlanetStatus[] }>('get_planets'),
  addPlanet: (source: string) => cmd('add_planet', { source }),
  removePlanet: (id: string) => cmd('remove_planet', { id }),
  enablePlanet: (id: string) => cmd('enable_planet', { id }),
  disablePlanet: (id: string) => cmd('disable_planet', { id }),
  networks: (planet?: string) => cmd<{ networks: NetworkEntry[] }>('get_networks', { planet: planet ?? null }),
  joinNetwork: (nwid: string, planet?: string) => cmd('join_network', { nwid, planet: planet ?? null }),
  leaveNetwork: (nwid: string, planet?: string) => cmd('leave_network', { nwid, planet: planet ?? null }),
  setNetwork: (
    nwid: string,
    body: { routeViaZeroTier?: boolean; dnsMode?: string; customDns?: string[] },
    planet?: string,
  ) => cmd('set_network', { nwid, body, planet: planet ?? null }),
  peers: (planet?: string) => cmd<{ peers: PeerEntry[] }>('get_peers', { planet: planet ?? null }),
};
