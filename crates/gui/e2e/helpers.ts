import { spawn, type ChildProcess } from 'node:child_process';
import net from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// ESM(package.json type:module)无 __dirname,经 import.meta.url 推导
const here = path.dirname(fileURLToPath(import.meta.url));

/** 仓库根(gui/e2e → 上三级) */
const repoRoot = path.resolve(here, '..', '..', '..');

/** 各平台被测应用路径:mac 需 .app bundle(裸二进制无 Info.plist,
 * safaridriver 无法拉起);linux/win 用 release 裸二进制即可 */
export function appPath(): string {
  if (process.platform === 'darwin') {
    return path.join(repoRoot, 'target', 'release', 'bundle', 'macos', 'planet-port.app');
  }
  const bin = process.platform === 'win32' ? 'planet-port-gui.exe' : 'planet-port-gui';
  return path.join(repoRoot, 'target', 'release', bin);
}

/** daemon API(127.0.0.1:9998)是否可达——suite 自选分流依据:
 * 可达跑 online 断言,不可达跑 offline 断言,skip 不算失败,
 * 同一套代码 CI(无 daemon)与实机(有 daemon)均可执行 */
export async function daemonUp(): Promise<boolean> {
  return new Promise(resolve => {
    const s = net.connect(9998, '127.0.0.1');
    s.setTimeout(1000);
    s.on('connect', () => { s.destroy(); resolve(true); });
    s.on('error', () => resolve(false));
    s.on('timeout', () => { s.destroy(); resolve(false); });
  });
}

/** tauri-driver 进程(wdio hooks 起停;4444 端口对齐 webdriver remote)。
 *
 * 平台支持面:linux(WebKitWebDriver)/windows(msedgedriver)。
 * macOS 无官方路径——tauri-driver ≥2.0 编译期排除 mac,0.1.5 mac 分支
 * 烂尾(DRIVER_BINARY cfg 缺失编译不过),fork(tauri-driver-fork)亦无 mac。
 * mac 的回归覆盖 = 逻辑层单测 CI(gui-test.yml macos-15),E2E 明确缺席 */
export function startTauriDriver(): ChildProcess {
  if (process.platform === 'darwin') {
    throw new Error('E2E 不支持 macOS:tauri-driver 全系版本无 mac 支持,详见 helpers.ts 注释');
  }
  const bin = process.platform === 'win32' ? 'tauri-driver.exe' : 'tauri-driver';
  return spawn(bin, [], { stdio: 'ignore', detached: false });
}

export function stopTauriDriver(p: ChildProcess | undefined): void {
  if (p && !p.killed) p.kill('SIGTERM');
}
