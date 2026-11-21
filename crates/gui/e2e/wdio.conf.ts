import path from 'node:path';
import { fileURLToPath } from 'node:url';
import type { Options } from '@wdio/types';
import { appPath, startTauriDriver, stopTauriDriver } from './helpers';

const here = path.dirname(fileURLToPath(import.meta.url));

// E2E 分流:E2E_SUITE=offline(CI,无 daemon)只跑离线引导面;
// =online(实机,daemon 运行中)只跑主界面交互。缺省 auto:探测 daemon 自选
const suite = process.env.E2E_SUITE ?? 'auto';
const specs =
  suite === 'offline' || suite === 'online'
    ? [`${here}/specs/${suite}.spec.ts`]
    : [`${here}/specs/*.spec.ts`];

let driverProc: ReturnType<typeof startTauriDriver> | undefined;

export const config: Options.Testrunner = {
  runner: 'local',
  specs,
  maxInstances: 1,
  capabilities: [
    {
      maxInstances: 1,
      // Tauri WebDriver 约定:tauri:options.application 指向被测产物
      'tauri:options': { application: appPath() },
    },
  ],
  hostname: '127.0.0.1',
  port: 4444,
  path: '/',
  logLevel: 'error',
  waitforTimeout: 15000,
  connectionRetryTimeout: 30000,
  connectionRetryCount: 2,
  framework: 'mocha',
  mochaOpts: {
    ui: 'bdd',
    timeout: 90000,
  },
  reporters: ['spec'],
  onPrepare: () => {
    driverProc = startTauriDriver();
    // 给 tauridriver 一拍监听 4444(connectionRetry 兜底)
    return new Promise(r => setTimeout(r, 1000));
  },
  onComplete: () => {
    stopTauriDriver(driverProc);
  },
};
