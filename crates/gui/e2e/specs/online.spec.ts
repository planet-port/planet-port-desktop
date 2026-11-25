import { daemonUp } from '../helpers';

// daemon 运行中环境的主界面交互(实机路径)。daemon 不可达则整组 skip
// 批 E 锚点对齐:批 B/C 把首页/抽屉/弹窗迁 Tailwind + ui-x 后,旧语义类
// (.pgroup-head/.netcards/.table/.btn.primary/.planet-icon/.dialog-actions)
// 已不在 DOM;本文件改为 testid / data-variant / data-slot 锚点,断言语义不变。
// 顶栏「添加网络」入口已移至首页空卡(home-add-network);卡片⇄列表切换视图
// 已随批 C 移除(卡片网格为唯一视图),对应切换步骤随之删除。
describe('online main ui', () => {
  before(async function (this: Mocha.Context) {
    if (!(await daemonUp())) this.skip();
  });

  it('窗口标题为 Planet Port', async () => {
    expect(await browser.getTitle()).toBe('Planet Port');
  });

  it('单页首页加载行星分组头', async () => {
    // react-query 首拉 + 心跳收敛在线态,留足余量
    const head = await browser.$('[data-testid="home-planet-name"]');
    await head.waitForDisplayed({ timeout: 30000 });
    expect((await head.getText()).length).toBeGreaterThan(0);
  });

  it('行星分组头显示状态摘要', async () => {
    const status = await browser.$('[data-testid="home-planet-status"]');
    expect((await status.getText()).length).toBeGreaterThan(0);
  });

  // 前置:需至少一个 enabled 行星——home-add-network 空卡入口受 p.enabled 门控,
  // 禁用态(或无行星)下该入口不渲染(行为随批 C 迁移而来)
  it('顶栏加入行星/设置入口 + 首页添加网络空卡在位', async () => {
    await expect(browser.$('[data-testid="topbar-add-planet"]')).toBeDisplayed();
    await expect(browser.$('[data-testid="topbar-settings"]')).toBeDisplayed();
    await expect(browser.$('[data-testid="home-add-network"]')).toBeDisplayed();
  });

  it('语言切换后行星分组仍在(zh ↔ en 不破布局)', async () => {
    await browser.$('[data-testid="topbar-settings"]').click();
    await browser.$('[data-testid="settings-drawer"]').waitForDisplayed({ timeout: 5000 });
    await browser.$('[data-testid="settings-lang-row"]').click();
    await browser.$('[data-testid="lang"]').waitForDisplayed({ timeout: 5000 });
    await browser.$('[data-testid="lang-en"]').click();
    await browser.$('[data-testid="lang"]').waitForDisplayed({ reverse: true });
    await expect(browser.$('[data-testid="home-planet-name"]')).toBeDisplayed();
    await browser.$('[data-testid="settings-lang-row"]').click();
    await browser.$('[data-testid="lang"]').waitForDisplayed({ timeout: 5000 });
    await browser.$('[data-testid="lang-zh"]').click();
    await browser.$('[data-testid="lang"]').waitForDisplayed({ reverse: true });
    await expect(browser.$('[data-testid="home-planet-name"]')).toBeDisplayed();
    // 关抽屉还原环境:mask 覆盖全屏,不关会拦住后续用例的顶栏点击
    await browser.$('[data-slot="sheet-overlay"]').click();
    await browser.$('[data-testid="settings-drawer"]').waitForDisplayed({ reverse: true });
  });

  // —— 增补交互冒烟(2026-11-22 新需求面;入口级断言,不改 daemon 数据) ——
  it('添加网络抽屉:非法 ID 禁用提交,遮罩可关', async () => {
    await browser.$('[data-testid="home-add-network"]').click();
    await browser.$('[data-testid="add-network-drawer"]').waitForDisplayed({ timeout: 5000 });
    await browser.$('[data-testid="add-network-drawer"] input').setValue('xyz');
    const join = await browser.$('[data-testid="add-network-drawer"] button[data-variant="primary"]');
    expect(await join.getAttribute('disabled')).toBe('true');
    await browser.$('[data-slot="sheet-overlay"]').click();
    await browser.$('[data-testid="add-network-drawer"]').waitForDisplayed({ reverse: true });
  });

  it('添加行星抽屉:图标预览在位,未填名称禁用提交', async () => {
    await browser.$('[data-testid="topbar-add-planet"]').click();
    await browser.$('[data-testid="add-planet-drawer"]').waitForDisplayed({ timeout: 5000 });
    await expect(browser.$('[data-testid="add-planet-drawer"] [data-testid="planet-icon"]')).toBeDisplayed();
    const add = await browser.$('[data-testid="add-planet-drawer"] button[data-variant="primary"]');
    expect(await add.getAttribute('disabled')).toBe('true');
    await browser.$('[data-slot="sheet-overlay"]').click();
    await browser.$('[data-testid="add-planet-drawer"]').waitForDisplayed({ reverse: true });
  });

  it('网络详情与行星设置入口(有数据时):卡片开详情/分组头⚙开设置', async () => {
    const card = await browser.$('[data-testid="network-card"]');
    if (!(await card.isDisplayed())) return; // 无网络环境跳过详情部分
    await card.click();
    await browser.$('[data-testid="network-detail"]').waitForDisplayed({ timeout: 5000 });
    await expect(browser.$('[data-testid="network-detail"] .kv .mono')).toBeDisplayed();
    // 右上角关闭钮收弹层(编辑即时生效,无保存按钮;语言无关选择器)
    await browser.$('[data-testid="network-detail"] [data-slot="dialog-close"]').click();
    await browser.$('[data-testid="network-detail"]').waitForDisplayed({ reverse: true });
    await browser.$('main header button[data-variant="icon"]').click(); // 分组头 ⚙
    await browser.$('[data-testid="planet-settings"]').waitForDisplayed({ timeout: 5000 });
    await expect(browser.$('[data-testid="planet-settings"] .kv .mono')).toBeDisplayed(); // World ID 行
    await browser.$('[data-testid="planet-settings"] [data-slot="dialog-close"]').click(); // 关闭
    await browser.$('[data-testid="planet-settings"]').waitForDisplayed({ reverse: true });
  });
});
