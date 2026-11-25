/// 首启向导纯逻辑:版本比较与自动安装区判定(独立于组件,便于单测)
/// 与 CLI install.rs version_lt 同语义

export type InstallPhase = 'idle' | 'installing' | 'failed';

export interface VersionTriple {
  core: [number, number, number];
  beta: number;
  dev: boolean;
}

/// 解析 X.Y.Z / X.Y.Z-beta.N / X.Y.Z-beta.N-dev 形态:
/// 正式版(无预发布段)beta 序号取 MAX_SAFE_INTEGER,天然高于一切 beta.N;
/// 非常规输入(非数字段)逐项退化为 0
function parseVersion(v: string): VersionTriple {
  const dash = v.indexOf('-');
  const coreStr = dash < 0 ? v : v.slice(0, dash);
  const rest = dash < 0 ? null : v.slice(dash + 1);
  const num = (s: string | undefined): number => {
    const n = Number.parseInt(s ?? '', 10);
    return Number.isNaN(n) ? 0 : n;
  };
  const segs = coreStr.split('.');
  const core: [number, number, number] = [num(segs[0]), num(segs[1]), num(segs[2])];
  if (rest === null) return { core, beta: Number.MAX_SAFE_INTEGER, dev: false };
  const dev = rest.endsWith('-dev');
  const stripped = dev ? rest.slice(0, -'-dev'.length) : rest;
  return { core, beta: num(stripped.split('.').pop()), dev };
}

/// a < b ? 序取 SemVer 语义:正式版 > 一切 beta;同 beta 号时 -dev 最低(beta.2-dev < beta.2);
/// 相等返回 false(调用方已按同版本短路)。只服务安装/升级判定,不追求完整版本序
export function versionLt(a: string, b: string): boolean {
  const pa = parseVersion(a);
  const pb = parseVersion(b);
  for (let i = 0; i < 3; i++) {
    if (pa.core[i] !== pb.core[i]) return pa.core[i] < pb.core[i];
  }
  if (pa.beta !== pb.beta) return pa.beta < pb.beta;
  return pa.dev && !pb.dev;
}

/// 自动安装区显隐判定:
/// - 无内嵌资源(manifest=null)→ 不显示,走手动引导;
/// - 未安装 → 显示「立即安装」;
/// - 已装但拿不到已装版本号 → 按幂等升级处理,仍显示;
/// - 已装且版本不低于内嵌版本 → 不显示(直接用手动「启动服务」引导)
/// - 已装且内嵌版本更高才显示升级区
export function needInstall(
  manifest: { daemon: string } | null,
  installed: boolean,
  installedVersion: string | null,
): boolean {
  if (manifest === null) return false;
  if (!installed) return true;
  if (!installedVersion) return true;
  return versionLt(installedVersion, manifest.daemon);
}
