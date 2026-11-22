import { daemonUp } from '../helpers';

// daemon 运行中环境的主界面交互(实机路径)。daemon 不可达则整组 skip
describe('online main ui', () => {
  before(async function (this: Mocha.Context) {
    if (!(await daemonUp())) this.skip();
  });

  it('窗口标题为 planet-port', async () => {
    expect(await browser.getTitle()).toBe('planet-port');
  });

  it('单页首页加载行星分组头(.pgroup)', async () => {
    // react-query 首拉 + 心跳收敛在线态,留足余量
    const head = await browser.$('.pgroup-head .pgroup-name');
    await head.waitForDisplayed({ timeout: 30000 });
    expect((await head.getText()).length).toBeGreaterThan(0);
  });

  it('行星分组头显示状态摘要', async () => {
    const small = await browser.$('.pgroup-head .small');
    expect((await small.getText()).length).toBeGreaterThan(0);
  });

  it('顶栏三操作按钮 + 卡片⇄列表切换', async () => {
    await expect(browser.$('[data-testid="topbar-add-network"]')).toBeDisplayed();
    await expect(browser.$('[data-testid="topbar-add-planet"]')).toBeDisplayed();
    await expect(browser.$('[data-testid="topbar-settings"]')).toBeDisplayed();
    // 行星下无网络时两种视图容器都不渲染(.empty 占位),此时仅校验顶栏入口
    if (!(await browser.$('.netcards .netcard, .table').isDisplayed())) return;
    await browser.$('.viewtoggle button[aria-label="list"]').click();
    await expect(browser.$('.table')).toBeDisplayed();
    await browser.$('.viewtoggle button[aria-label="cards"]').click();
    await expect(browser.$('.netcards')).toBeDisplayed();
  });

  it('语言切换后行星分组仍在(zh ↔ en 不破布局)', async () => {
    await browser.$('[data-testid="topbar-settings"]').click();
    await browser.$('[data-testid="settings-drawer"]').waitForDisplayed({ timeout: 5000 });
    await browser.$('[data-testid="lang-en"]').click();
    await expect(browser.$('.pgroup-head .pgroup-name')).toBeDisplayed();
    await browser.$('[data-testid="lang-zh"]').click();
    await expect(browser.$('.pgroup-head .pgroup-name')).toBeDisplayed();
    // 关抽屉还原环境:mask 覆盖全屏,不关会拦住后续用例的顶栏点击
    await browser.$('.drawer-mask').click();
    await browser.$('[data-testid="settings-drawer"]').waitForDisplayed({ reverse: true });
  });

  // —— 增补交互冒烟(2026-11-22 新需求面;入口级断言,不改 daemon 数据) ——
  it('添加网络抽屉:非法 ID 禁用提交,遮罩可关', async () => {
    await browser.$('[data-testid="topbar-add-network"]').click();
    await browser.$('[data-testid="add-network-drawer"]').waitForDisplayed({ timeout: 5000 });
    await browser.$('[data-testid="add-network-drawer"] input').setValue('xyz');
    const join = await browser.$('[data-testid="add-network-drawer"] .btn.primary');
    expect(await join.getAttribute('disabled')).toBe('true');
    await browser.$('.drawer-mask').click();
    await browser.$('[data-testid="add-network-drawer"]').waitForDisplayed({ reverse: true });
  });

  it('添加行星抽屉:图标预览在位,未填名称禁用提交', async () => {
    await browser.$('[data-testid="topbar-add-planet"]').click();
    await browser.$('[data-testid="add-planet-drawer"]').waitForDisplayed({ timeout: 5000 });
    await expect(browser.$('[data-testid="add-planet-drawer"] .planet-icon')).toBeDisplayed();
    const add = await browser.$('[data-testid="add-planet-drawer"] .btn.primary');
    expect(await add.getAttribute('disabled')).toBe('true');
    await browser.$('.drawer-mask').click();
    await browser.$('[data-testid="add-planet-drawer"]').waitForDisplayed({ reverse: true });
  });

  it('网络详情与行星设置入口(有数据时):卡片开详情/分组头⚙开设置', async () => {
    // 前序用例可能把视图切到列表态(netview 存 localStorage),先归位卡片视图
    await browser.$('.viewtoggle button[aria-label="cards"]').click();
    const card = await browser.$('.netcards .netcard');
    if (!(await card.isDisplayed())) return; // 无网络环境跳过详情部分
    await card.click();
    await browser.$('[data-testid="network-detail"]').waitForDisplayed({ timeout: 5000 });
    await expect(browser.$('[data-testid="network-detail"] .kv .mono')).toBeDisplayed();
    // 保存按钮关闭弹层(提交同值 setNetwork,无副作用;语言无关选择器)
    await browser.$('[data-testid="network-detail"] .dialog-actions .btn.primary').click();
    await browser.$('[data-testid="network-detail"]').waitForDisplayed({ reverse: true });
    await browser.$('.pgroup-head .btn.icon').click(); // 分组头 ⚙
    await browser.$('[data-testid="planet-settings"]').waitForDisplayed({ timeout: 5000 });
    await expect(browser.$('[data-testid="planet-settings"] .kv .mono')).toBeDisplayed(); // World ID 行
    await browser.$('[data-testid="planet-settings"] .dialog-actions .btn:not(.danger)').click(); // 关闭
    await browser.$('[data-testid="planet-settings"]').waitForDisplayed({ reverse: true });
  });
});
