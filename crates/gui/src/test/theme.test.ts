import { afterEach, describe, expect, it, vi } from 'vitest';

/// matchMedia mock(system 态监听依赖;jsdom 未实现)
function mockMatchMedia(dark: boolean) {
  vi.stubGlobal('matchMedia', (q: string) => ({
    matches: q.includes('dark') ? dark : false,
    addEventListener: () => {},
    removeEventListener: () => {},
  }));
}

describe('theme', () => {
  afterEach(() => {
    localStorage.clear();
    document.documentElement.removeAttribute('data-theme');
    vi.unstubAllGlobals();
  });

  it('resolveTheme: 显式模式直取,system 随系统', async () => {
    const { resolveTheme } = await import('../theme');
    expect(resolveTheme('light', true)).toBe('light');
    expect(resolveTheme('dark', false)).toBe('dark');
    expect(resolveTheme('system', true)).toBe('dark');
    expect(resolveTheme('system', false)).toBe('light');
  });

  it('initTheme: 无存储时默认 system 并按系统深色落 data-theme', async () => {
    mockMatchMedia(true);
    const { initTheme } = await import('../theme');
    initTheme();
    expect(localStorage.getItem('theme')).toBe('system');
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
  });

  it('setTheme: 持久化并即时应用(静态,无过渡类)', async () => {
    mockMatchMedia(false);
    const { initTheme, setTheme } = await import('../theme');
    initTheme();
    setTheme('dark');
    expect(localStorage.getItem('theme')).toBe('dark');
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
    setTheme('light');
    expect(document.documentElement.getAttribute('data-theme')).toBe('light');
  });
});
