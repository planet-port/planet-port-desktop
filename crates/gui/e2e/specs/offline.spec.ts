import { daemonUp } from '../helpers';

// daemon 不可达环境的离线形态(CI 默认路径):首页常驻只禁用,不再有引导页。
// daemon 在跑则整组 skip——skip 不是失败,同一套代码在实机(online)环境不误报
describe('offline', () => {
  before(async function (this: Mocha.Context) {
    if (await daemonUp()) this.skip();
  });

  it('窗口标题为 Planet Port', async () => {
    const title = await browser.getTitle();
    expect(title).toBe('Planet Port');
  });

  it('首页常驻:顶栏设置可见,添加行星禁用(不再整页替换为引导页)', async () => {
    // 设置常显(安装入口在内);添加行星依赖 daemon,离线禁用。
    await browser.$('[data-testid="topbar-settings"]').waitForDisplayed({ timeout: 20000 });
    // 给心跳首探(3s 周期)离线确认留余量
    await expect(browser.$('[data-testid="topbar-add-planet"]')).not.toBeEnabled();
  });

  it('离线仍可开设置抽屉;无内嵌资源时无安装入口', async () => {
    // CI 走 --no-bundle 且无 resources 目录 → embedded_manifest 返回 null →
    // 安装行(DaemonInstallRow)不渲染,离线也没有引导页兜底安装
    await browser.$('[data-testid="topbar-settings"]').click();
    await browser.$('[data-testid="settings-drawer"]').waitForDisplayed({ timeout: 5000 });
    await expect(browser.$('[data-testid="settings-install-daemon"]')).not.toBeDisplayed();
    // 关抽屉还原环境:mask 覆盖全屏,不关会拦住后续用例的顶栏点击
    await browser.$('[data-slot="sheet-overlay"]').click();
    await browser.$('[data-testid="settings-drawer"]').waitForDisplayed({ reverse: true });
  });
});

// 设置抽屉冒烟:语言/主题属在线实机路径(与上方离线组按 daemon 状态互斥,
// skip 不算失败);抽屉本身离线也可挂载,安装能力在内
describe('settings drawer', () => {
  before(async function (this: Mocha.Context) {
    if (!(await daemonUp())) this.skip();
  });

  it('设置抽屉开合与语言切换可用', async () => {
    await browser.$('[data-testid="topbar-settings"]').click();
    await browser.$('[data-testid="settings-drawer"]').waitForDisplayed({ timeout: 5000 });
    // 语言选择在二级勾选页(CheckListSheet):值行进页,选中项右侧 ✓(.check);
    // 批 E 锚点对齐:旧 .lang-*.active 类已随抽屉/弹窗迁移移除,选项即选即关
    await browser.$('[data-testid="settings-lang-row"]').click();
    await browser.$('[data-testid="lang"]').waitForDisplayed({ timeout: 5000 });
    // 选中项右侧 ✓(.check,语义类仍由 styles.css 保留)对位旧 .active 断言;
    // 选项即选即关,每次切换后重开勾选页才能断选中态(旧用例勾选页保持打开)
    await expect(browser.$('[data-testid="lang-zh"] .check')).toBeDisplayed();
    await browser.$('[data-testid="lang-en"]').click();
    await browser.$('[data-testid="lang"]').waitForDisplayed({ reverse: true });
    await browser.$('[data-testid="settings-lang-row"]').click();
    await browser.$('[data-testid="lang"]').waitForDisplayed({ timeout: 5000 });
    await expect(browser.$('[data-testid="lang-en"] .check')).toBeDisplayed();
    await browser.$('[data-testid="lang-zh"]').click();
    await browser.$('[data-testid="lang"]').waitForDisplayed({ reverse: true });
    await browser.$('[data-testid="settings-lang-row"]').click();
    await browser.$('[data-testid="lang"]').waitForDisplayed({ timeout: 5000 });
    await expect(browser.$('[data-testid="lang-zh"] .check')).toBeDisplayed();
    // 右上角关闭钮收勾选页(不触发任何选项)
    await browser.$('[data-slot="dialog-close"]').click();
    await browser.$('[data-testid="lang"]').waitForDisplayed({ reverse: true });
    // 关抽屉还原环境:mask 覆盖全屏,不关会拦住后续用例的顶栏点击
    await browser.$('[data-slot="sheet-overlay"]').click();
    await browser.$('[data-testid="settings-drawer"]').waitForDisplayed({ reverse: true });
  });

  it('主题切换:抽屉内切深色后 html 带 data-theme=dark', async () => {
    await browser.$('[data-testid="topbar-settings"]').click();
    await browser.$('[data-testid="settings-drawer"]').waitForDisplayed({ timeout: 5000 });
    await browser.$('[data-testid="settings-appearance-row"]').click();
    await browser.$('[data-testid="theme"]').waitForDisplayed({ timeout: 5000 });
    await browser.$('[data-testid="theme-dark"]').click();
    await browser.$('[data-testid="theme"]').waitForDisplayed({ reverse: true });
    const attr = await browser.execute(() => document.documentElement.getAttribute('data-theme'));
    expect(attr).toBe('dark');
    await browser.$('[data-testid="settings-appearance-row"]').click();
    await browser.$('[data-testid="theme"]').waitForDisplayed({ timeout: 5000 });
    await browser.$('[data-testid="theme-system"]').click(); // 还原默认
    await browser.$('[data-testid="theme"]').waitForDisplayed({ reverse: true });
    await browser.$('[data-slot="sheet-overlay"]').click();
    await browser.$('[data-testid="settings-drawer"]').waitForDisplayed({ reverse: true });
  });
});
