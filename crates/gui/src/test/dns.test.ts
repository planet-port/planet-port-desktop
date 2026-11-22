import { describe, expect, it } from 'vitest';
import { emptySlots, isIPv4, isIPv6, serversToSlots, slotsToServers, slotsValid } from '../dns';

describe('DNS 四槽模型', () => {
  it('IPv4 校验:点分四段且各段 ≤255', () => {
    expect(isIPv4('10.121.17.1')).toBe(true);
    expect(isIPv4('10.121.17.256')).toBe(false);
    expect(isIPv4('10.121.17')).toBe(false);
  });
  it('IPv6 校验:冒号十六进制,允许 :: 压缩一次', () => {
    expect(isIPv6('fd00::1')).toBe(true);
    expect(isIPv6('fe80:0000:0000:0000:0000:0000:0000:0001')).toBe(true);
    expect(isIPv6('::1')).toBe(true);
    expect(isIPv6('fd00::1::2')).toBe(false);
    expect(isIPv6(':::')).toBe(false);
    expect(isIPv6('a::b:')).toBe(false);
    expect(isIPv6(':a::b')).toBe(false);
    expect(isIPv6('1:2:3:4:5:6:7:8:9')).toBe(false);
    expect(isIPv6('10.0.0.1')).toBe(false);
  });
  it('slotsValid:custom 至少一个且槽位格式合法', () => {
    expect(slotsValid(emptySlots())).toBe(false);
    expect(slotsValid({ ...emptySlots(), v4a: '10.0.0.1' })).toBe(true);
    expect(slotsValid({ ...emptySlots(), v6a: 'fd00::1' })).toBe(true);
    expect(slotsValid({ ...emptySlots(), v4a: '300.0.0.1' })).toBe(false);
    expect(slotsValid({ ...emptySlots(), v4a: '10.0.0.1', v6b: 'zz::9' })).toBe(false);
  });
  it('slotsToServers/serversToSlots 往返:v4 在前的线格式', () => {
    const s = { v4a: '10.0.0.1', v4b: '', v6a: 'fd00::1', v6b: 'fd00::2' };
    expect(slotsToServers(s)).toEqual(['10.0.0.1', 'fd00::1', 'fd00::2']);
    expect(serversToSlots(['10.0.0.1', 'fd00::1', 'fd00::2'])).toEqual(s);
    expect(serversToSlots([])).toEqual(emptySlots());
  });
});
