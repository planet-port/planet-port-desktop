/// 主题三态:light/dark 显式、system 跟随系统(data-theme 由 matchMedia 决定)。
/// 静态应用无过渡动效(spec「不需要视差/动效」)。
export type ThemeMode = 'light' | 'dark' | 'system';

export function resolveTheme(mode: ThemeMode, systemDark: boolean): 'light' | 'dark' {
  if (mode === 'system') return systemDark ? 'dark' : 'light';
  return mode;
}

function apply(mode: ThemeMode) {
  const systemDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
  document.documentElement.setAttribute('data-theme', resolveTheme(mode, systemDark));
}

/// 应用启动调用一次:读存储 + 应用 + system 态挂系统变化监听
export function initTheme() {
  const saved = localStorage.getItem('theme');
  const mode: ThemeMode = saved === 'light' || saved === 'dark' ? saved : 'system';
  localStorage.setItem('theme', mode);
  apply(mode);
  window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
    if (getTheme() === 'system') apply('system');
  });
}

export function setTheme(mode: ThemeMode) {
  localStorage.setItem('theme', mode);
  apply(mode);
}

export function getTheme(): ThemeMode {
  const saved = localStorage.getItem('theme');
  return saved === 'light' || saved === 'dark' ? saved : 'system';
}
