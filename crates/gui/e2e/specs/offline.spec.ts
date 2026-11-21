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

  it('语言切换可用(zh ↔ en)', async () => {
    const zh = await browser.$('.lang button:nth-child(1)');
    const en = await browser.$('.lang button:nth-child(2)');
    await en.click();
    await expect(browser.$('.lang button:nth-child(2).active')).toBeDisplayed();
    await zh.click();
    await expect(browser.$('.lang button:nth-child(1).active')).toBeDisplayed();
  });
});
