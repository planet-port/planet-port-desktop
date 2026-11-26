/// 版本比较与内嵌/已装差异判定(纯函数,便于单测)。
/// versionLt 与 CLI install.rs version_lt 同语义,自 pages/setup.ts 迁入
/// (SetupWizard 随离线引导页移除,首启向导语境不复存在)

export type VersionRelation = 'older' | 'newer' | 'equal' | 'unknown';

/// 单组件比对:null = 任一侧探测不到,如实标 unknown 不猜
export function compareVersion(installed: string | null, embedded: string | null): VersionRelation {
  if (!installed || !embedded) return 'unknown';
  if (installed === embedded) return 'equal';
  return versionLt(installed, embedded) ? 'older' : 'newer';
}

/// 整体状态:newer(已装新于内嵌,GUI 未经该组合测试)优先于 older(可升级);
/// 双 unknown 无从判定 → silent(调用方不提示);其余组合落 even
export type OverallVersionState = 'upgrade' | 'compat' | 'even' | 'silent';
export function overallState(daemon: VersionRelation, cli: VersionRelation): OverallVersionState {
  if (daemon === 'newer' || cli === 'newer') return 'compat';
  if (daemon === 'older' || cli === 'older') return 'upgrade';
  if (daemon === 'unknown' && cli === 'unknown') return 'silent';
  return 'even';
}

// ↓↓↓ 自 pages/setup.ts 原样迁入,实现不动 ↓↓↓
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
