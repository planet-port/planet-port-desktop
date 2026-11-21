import { daemonUp } from '../helpers';

// daemon 运行中环境的主界面交互(实机路径)。daemon 不可达则整组 skip
describe('online main ui', () => {
  before(async function (this: Mocha.Context) {
    if (!(await daemonUp())) this.skip();
  });

  it('窗口标题为 planet-port', async () => {
    expect(await browser.getTitle()).toBe('planet-port');
  });

  it('总览页加载 planet 卡片(Overview .card)', async () => {
    // react-query 首拉 + 心跳收敛在线态,留足余量
    const card = await browser.$('.page .cards .card');
    await card.waitForDisplayed({ timeout: 30000 });
    const name = await browser.$('.page .cards .card .name').getText();
    expect(name.length).toBeGreaterThan(0);
  });

  it('planet 卡片显示运行状态徽标', async () => {
    const badge = await browser.$('.page .cards .card .badge').getText();
    expect(badge.length).toBeGreaterThan(0);
  });

  it('导航切换四页均渲染(.page 存在)', async () => {
    const buttons = await browser.$$('nav button');
    expect(buttons.length).toBe(4);
    for (const b of buttons) {
      await b.click();
      await expect(browser.$('.page')).toBeDisplayed();
    }
  });

  it('语言切换后 planet 卡片仍在(zh ↔ en 不破布局)', async () => {
    const en = await browser.$('.lang button:nth-child(2)');
    const zh = await browser.$('.lang button:nth-child(1)');
    await en.click();
    await expect(browser.$('.page .cards .card')).toBeDisplayed();
    await zh.click();
    await expect(browser.$('.page .cards .card')).toBeDisplayed();
  });
});
