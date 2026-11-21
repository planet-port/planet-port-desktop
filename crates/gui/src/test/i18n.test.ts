import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import en from '../i18n/en';
import zh from '../i18n/zh';

/// 迁移自仓库 tmp/check-i18n.mjs 的口径：zh/en 键集合一致 + 字面 t() 键零缺失
/// 注意 jsdom 环境的全局 URL 是 jsdom 实现，不能传 URL 实例给 fileURLToPath，只能传字符串
const srcRoot = resolve(fileURLToPath(import.meta.url), '..', '..');

const flatKeys = (o: Record<string, unknown>, prefix = ''): string[] =>
  Object.entries(o).flatMap(([k, v]) =>
    typeof v === 'object' && v !== null ? flatKeys(v as Record<string, unknown>, prefix ? `${prefix}.${k}` : k) : [prefix ? `${prefix}.${k}` : k],
  );

const zhKeys = flatKeys(zh);
const enKeys = flatKeys(en);

/// 运行时模板拼接的键无法静态扫描，改为断言动态前缀的枚举闭集齐全
const dynamicPrefixes: Record<string, string[]> = {
  nav: ['overview', 'networks', 'planets', 'peers'], // App.tsx: t(`nav.${p}`)
  'planet.status': ['running', 'stopped'], // Overview/Planets: t(`planet.status.${p.status}`)
  'planet.kind': ['official', 'custom'], // Overview/Planets: t(`planet.kind.${p.type}`)
  'transport.tcp': ['disabled', 'idle', 'connecting', 'established'], // Overview: t(`transport.tcp.${t.tcp}`)
};

/// 收集 src 下全部 ts/tsx（跳过测试自身与资源文件，避免自引用）
function collectFiles(dir: string): string[] {
  const out: string[] = [];
  for (const f of readdirSync(dir)) {
    const p = join(dir, f);
    const rel = relative(srcRoot, p);
    const top = rel.split(sep)[0];
    if (top === 'test' || top === 'i18n') continue;
    if (statSync(p).isDirectory()) out.push(...collectFiles(p));
    else if (/\.(ts|tsx)$/.test(f)) out.push(p);
  }
  return out;
}

describe('i18n 资源契约', () => {
  it('zh 与 en 扁平键集合深度相等', () => {
    const onlyZh = zhKeys.filter(k => !enKeys.includes(k));
    const onlyEn = enKeys.filter(k => !zhKeys.includes(k));
    expect({ onlyZh, onlyEn }).toEqual({ onlyZh: [], onlyEn: [] });
    expect([...zhKeys].sort()).toEqual([...enKeys].sort());
  });

  it('字面 t(\'...\') 键全部存在于 zh（动态模板键跳过）', () => {
    const files = collectFiles(srcRoot);
    expect(files.length).toBeGreaterThan(0);
    const missing: string[] = [];
    for (const f of files) {
      const src = readFileSync(f, 'utf8');
      for (const m of src.matchAll(/\bt\(\s*['"`]([^'"`\n]+?)['"`]/g)) {
        const key = m[1];
        if (key.includes('${')) continue; // 模板插值键由闭集断言覆盖
        if (!zhKeys.includes(key)) missing.push(`${relative(srcRoot, f)}: t('${key}')`);
      }
    }
    expect(missing).toEqual([]);
  });

  it('动态前缀四组枚举闭集在 zh/en 均齐全', () => {
    for (const [prefix, leaves] of Object.entries(dynamicPrefixes)) {
      for (const leaf of leaves) {
        const key = `${prefix}.${leaf}`;
        expect(zhKeys, `zh 缺少 ${key}`).toContain(key);
        expect(enKeys, `en 缺少 ${key}`).toContain(key);
      }
    }
  });
});
