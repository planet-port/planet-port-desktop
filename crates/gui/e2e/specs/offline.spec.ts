import { daemonUp } from '../helpers';

// daemon 不可达环境的离线引导面(CI 默认路径)。daemon 在跑则整组 skip
// ——skip 不是失败,同一套代码在实机(online)环境不误报
describe('offline guide', () => {
  before(async function (this: Mocha.Context) {
    if (await daemonUp()) this.skip();
  });

  it('窗口标题为 planet-port', async () => {
    const title = await browser.getTitle();
    expect(title).toBe('planet-port');
  });

  it('离线引导页可见(OfflineGuide .guide)', async () => {
    // OfflineGuide 根节点 .guide;给心跳首探(3s 周期)离线确认留余量
    await browser.$('.guide').waitForDisplayed({ timeout: 20000 });
  });

  it('无内嵌资源时降级手动引导(不出现自动安装按钮)', async () => {
    // CI 走 --no-bundle 且无 resources 目录 → embedded_manifest 返回 null →
    // OfflineGuide 退回纯手动引导,不挂载 SetupWizard,install-btn 不应存在
    await expect(browser.$('[data-testid="install-btn"]')).not.toBeDisplayed();
  });
});

// 设置抽屉冒烟:抽屉只在 daemon 在线(App online 态)时挂载,与上方离线引导组
// 互斥——本组 daemon 不可达时整组 skip(skip 不算失败,实机路径才真正执行)
describe('settings drawer', () => {
  before(async function (this: Mocha.Context) {
    if (!(await daemonUp())) this.skip();
  });

  it('设置抽屉开合与语言切换可用', async () => {
    await browser.$('[data-testid="topbar-settings"]').click();
    await browser.$('[data-testid="settings-drawer"]').waitForDisplayed({ timeout: 5000 });
    await browser.$('[data-testid="lang-en"]').click();
    await expect(browser.$('[data-testid="lang-en"].active')).toBeDisplayed();
    await browser.$('[data-testid="lang-zh"]').click();
    await expect(browser.$('[data-testid="lang-zh"].active')).toBeDisplayed();
    // 关抽屉还原环境:mask 覆盖全屏,不关会拦住后续用例的顶栏点击
    await browser.$('.drawer-mask').click();
    await browser.$('[data-testid="settings-drawer"]').waitForDisplayed({ reverse: true });
  });

  it('主题切换:抽屉内切深色后 html 带 data-theme=dark', async () => {
    await browser.$('[data-testid="topbar-settings"]').click();
    await browser.$('[data-testid="settings-drawer"]').waitForDisplayed({ timeout: 5000 });
    await browser.$('[data-testid="theme-dark"]').click();
    const attr = await browser.execute(() => document.documentElement.getAttribute('data-theme'));
    expect(attr).toBe('dark');
    await browser.$('[data-testid="theme-system"]').click(); // 还原默认
    await browser.$('.drawer-mask').click();
    await browser.$('[data-testid="settings-drawer"]').waitForDisplayed({ reverse: true });
  });
});
