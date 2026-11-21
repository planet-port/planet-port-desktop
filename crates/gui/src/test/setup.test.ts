import { describe, expect, it } from 'vitest';
import { needInstall, versionLt } from '../pages/setup';

/// 与 CLI install.rs version_lt 同语义:正式版 > 一切 beta;同 beta 号时 -dev 最低;
/// 非常规输入逐段退化为 0;相等返回 false
describe('versionLt', () => {
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

/// 自动安装区显隐判定:无内嵌资源不显示;未装显示;已装但无已装版本号按幂等升级显示;
/// 已装且有版本号时按 versionLt 判升级
describe('needInstall', () => {
  const m = { version: '0.5.0', target: 'aarch64-apple-darwin' };

  it('manifest 为 null(无内嵌资源)不出现自动安装区', () => {
    expect(needInstall(null, false, null)).toBe(false);
    expect(needInstall(null, true, '0.4.0')).toBe(false);
  });

  it('未安装即显示自动安装区', () => {
    expect(needInstall(m, false, null)).toBe(true);
  });

  it('已装但拿不到已装版本号:按幂等升级处理,仍显示', () => {
    expect(needInstall(m, true, null)).toBe(true);
  });

  it('已装版本低于内嵌版本(含 beta 形态)显示升级区', () => {
    expect(needInstall(m, true, '0.4.9')).toBe(true);
    expect(needInstall(m, true, '0.5.0-beta.1')).toBe(true);
  });

  it('已装版本不低于内嵌版本时不显示', () => {
    expect(needInstall(m, true, '0.5.0')).toBe(false);
    expect(needInstall(m, true, '0.5.1')).toBe(false);
  });
});
