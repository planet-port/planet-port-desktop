import { convertFileSrc } from '@tauri-apps/api/core';

/// 背景图:Rust 侧复制进 appData/background(路径经 asset 协议引用),
/// 前端只记 appData 绝对路径。应用时设 --bg-image 并挂 html.has-bg
/// (CSS 以半透明 --bg 蒙层保证前景可读性);null = 清除。
export function initBackground() {
  applyBg();
}

export function setBackground(appDataPath: string | null) {
  if (appDataPath) localStorage.setItem('background', appDataPath);
  else localStorage.removeItem('background');
  applyBg();
}

export function getBackground(): string | null {
  return localStorage.getItem('background');
}

/// 当前背景的 webview 可引用 URL(asset 协议);供设置里做缩略预览
export function backgroundUrl(): string | null {
  const p = getBackground();
  return p ? convertFileSrc(p) : null;
}

function applyBg() {
  const root = document.documentElement;
  const p = getBackground();
  if (p) {
    root.style.setProperty('--bg-image', `url(${convertFileSrc(p)})`);
    root.classList.add('has-bg');
  } else {
    root.style.removeProperty('--bg-image');
    root.classList.remove('has-bg');
  }
}
