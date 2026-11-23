import { describe, expect, it } from 'vitest';
import { ANIMALS, ANIMAL_SVGS, PALETTE, animalFor, colorFor, djb2, randomColorHex } from '../icons/planetIcon';

describe('行星图标体系(跨端契约)', () => {
  it('92 动物目录与移动端逐字一致(锚定 Android PlanetIcon.catalog)', () => {
    expect(ANIMALS.length).toBe(92);
    expect(ANIMALS[0]).toBe('ant');
    expect(ANIMALS[72]).toBe('seal');
    expect(ANIMALS[91]).toBe('worm');
    expect(new Set(ANIMALS).size).toBe(92);
  });

  it('djb2 向量:gozi25→seal、ZeroTier 官方→frog(与 iOS/Android 实测一致)', () => {
    expect(animalFor(null, 'gozi25')).toBe('seal');
    expect(animalFor(null, 'ZeroTier 官方')).toBe('frog');
    // 64 位回绕参与运算:此向量在 32 位截断实现下会落到别的 key
    expect(djb2('gozi25')).toBeGreaterThan(0);
  });

  it('animalFor:有效自定义 key 永久生效,无效旧值按名称哈希回退', () => {
    expect(animalFor('panda', 'gozi25')).toBe('panda');
    expect(animalFor('sf.symbol.legacy', 'gozi25')).toBe('seal');
    expect(animalFor('', 'gozi25')).toBe('seal');
  });

  it('colorFor:自定义 hex 原样(小写),回退落 8 色派生盘且同名稳定', () => {
    expect(colorFor('#DC2626', 'whatever')).toBe('#dc2626');
    expect(colorFor(null, 'gozi25')).toBe(colorFor(null, 'gozi25'));
    expect(colorFor(undefined, 'a')).not.toBe(colorFor(undefined, 'b'));
  });

  it('randomColorHex:恒为 #RRGGBB(随机底色域 S .45-.85 L .32-.55 的产物)', () => {
    for (let i = 0; i < 50; i++) expect(randomColorHex()).toMatch(/^#[0-9a-f]{6}$/);
  });

  it('PALETTE 24 色与 iOS IconPickerSheet 逐字一致', () => {
    expect(PALETTE.length).toBe(24);
    expect(PALETTE[0]).toBe('#dc2626');
    expect(PALETTE[23]).toBe('#334155');
  });

  it('ANIMAL_SVGS:92 动物全部映射到资产 url,zerotier 品牌特判不入 ANIMALS', () => {
    for (const k of ANIMALS) expect(ANIMAL_SVGS[k]).toBeTruthy();
    // zerotier 是官方行星品牌图标:animalFor 特判直返,不入 ANIMALS(表长参与
    // djb2 取模,追加会漂移全部回退图标),资产由 animals/ 目录 glob 收集
    expect(ANIMAL_SVGS.zerotier).toBeTruthy();
    expect(ANIMALS).not.toContain('zerotier');
    expect(animalFor('zerotier', 'whatever')).toBe('zerotier');
  });
});
