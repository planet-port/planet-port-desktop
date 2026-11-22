/// 主题三态:light/dark 显式、system 跟随系统(data-theme 由 matchMedia 决定)。
/// 静态应用无过渡动效(spec「不需要视差/动效」)。
/// 主题色(accent):用户自选主色覆盖 --primary(hover 亮暗由 CSS color-mix
/// 从 --primary 派生,null = 回落 CSS 内置默认色)。
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
  applyAccent();
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

/// 主题色:null = 恢复默认(移除内联覆盖,CSS 字面量回落)
function applyAccent() {
  const hex = localStorage.getItem('accent');
  if (hex) document.documentElement.style.setProperty('--primary', hex);
  else document.documentElement.style.removeProperty('--primary');
}

export function setAccent(hex: string | null) {
  if (hex) localStorage.setItem('accent', hex);
  else localStorage.removeItem('accent');
  applyAccent();
}

export function getAccent(): string | null {
  return localStorage.getItem('accent');
}
