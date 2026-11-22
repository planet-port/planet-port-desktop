/// 平台检测(webview UA 判别,离线引导与首页命令卡共用)
export function detectOs(): 'macos' | 'windows' | 'linux' {
  const ua = navigator.userAgent;
  if (ua.includes('Windows')) return 'windows';
  if (ua.includes('Mac')) return 'macos';
  return 'linux';
}
