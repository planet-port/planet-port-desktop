import { describe, expect, it } from 'vitest';
import { compareVersion, overallState, versionLt } from '../lib/version';

/// 与 CLI install.rs version_lt 同语义:正式版 > 一切 beta;同 beta 号时 -dev 最低;
/// 非常规输入逐段退化为 0;相等返回 false
describe('versionLt(自 setup.ts 迁移,用例原样)', () => {
  it('正式版点分比较:0.4.1 < 0.4.2', () => {
    expect(versionLt('0.4.1', '0.4.2')).toBe(true);
  });

  it('反向比较:0.4.2 不小于 0.4.1', () => {
    expect(versionLt('0.4.2', '0.4.1')).toBe(false);
  });

  it('beta 低于同核心号的正式版:0.4.1-beta.2 < 0.4.1', () => {
    expect(versionLt('0.4.1-beta.2', '0.4.1')).toBe(true);
    expect(versionLt('0.4.1', '0.4.1-beta.2')).toBe(false);
  });

  it('-dev 占位号低于同 beta 号:0.4.1-beta.2-dev < 0.4.1-beta.2', () => {
    expect(versionLt('0.4.1-beta.2-dev', '0.4.1-beta.2')).toBe(true);
    expect(versionLt('0.4.1-beta.2', '0.4.1-beta.2-dev')).toBe(false);
  });

  it('beta 序号按数值比较:beta.10 > beta.2(非字典序)', () => {
    expect(versionLt('0.4.1-beta.2', '0.4.1-beta.10')).toBe(true);
    expect(versionLt('0.4.1-beta.10', '0.4.1-beta.2')).toBe(false);
  });

  it('beta 核心号高于旧正式版:0.5.0-beta.1 不小于 0.4.9', () => {
    expect(versionLt('0.5.0-beta.1', '0.4.9')).toBe(false);
    expect(versionLt('0.4.9', '0.5.0-beta.1')).toBe(true);
  });

  it('完全相等返回 false', () => {
    expect(versionLt('0.4.1', '0.4.1')).toBe(false);
    expect(versionLt('0.4.1-beta.3', '0.4.1-beta.3')).toBe(false);
    expect(versionLt('0.4.1-beta.3-dev', '0.4.1-beta.3-dev')).toBe(false);
  });

  it('非常规输入逐段退化为 0,不抛异常', () => {
    expect(versionLt('', '0.0.1')).toBe(true);
    expect(versionLt('x.y', '0.0.1')).toBe(true); // x.y → 0.0.0 < 0.0.1
    expect(versionLt('0.4.1-beta.x', '0.4.1-beta.1')).toBe(true); // beta.x → 0
  });
});

describe('compareVersion', () => {
  it('任一侧为 null → unknown', () => {
    expect(compareVersion(null, '0.5.1')).toBe('unknown');
    expect(compareVersion('0.5.1', null)).toBe('unknown');
  });
  it('相等字符串 → equal', () => expect(compareVersion('0.5.1', '0.5.1')).toBe('equal'));
  it('已装旧于内嵌 → older(含 beta 序)', () => {
    expect(compareVersion('0.5.1', '0.6.0')).toBe('older');
    expect(compareVersion('0.6.0-beta.1', '0.6.0')).toBe('older');
  });
  it('已装新于内嵌 → newer', () => expect(compareVersion('0.6.1', '0.6.0')).toBe('newer'));
});

describe('overallState', () => {
  it('双 unknown → silent', () => expect(overallState('unknown', 'unknown')).toBe('silent'));
  it('newer 优先于 older(混合时报兼容)', () => {
    expect(overallState('newer', 'older')).toBe('compat');
    expect(overallState('older', 'newer')).toBe('compat');
  });
  it('任一 older → upgrade', () => {
    expect(overallState('older', 'equal')).toBe('upgrade');
    expect(overallState('unknown', 'older')).toBe('upgrade');
  });
  it('其余 → even', () => {
    expect(overallState('equal', 'equal')).toBe('even');
    expect(overallState('equal', 'unknown')).toBe('even');
  });
});
