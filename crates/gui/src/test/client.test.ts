import { describe, expect, it } from 'vitest';
import { asError } from '../api/client';

/// asError 是对 invoke reject 值的类型断言（原样透传），本组测试固化这一契约：
/// 结构化 GuiError 字段可读；非结构化 reject 值（string/Error/undefined）不被吞掉
describe('asError', () => {
  it('GuiError 形态对象透传，status/code/message/candidates 可读', () => {
    const ge = asError({
      kind: 'api',
      status: 400,
      code: 'network_not_found',
      message: 'unknown network',
      candidates: ['9536600adcf2b350'],
    });
    expect(ge.kind).toBe('api');
    expect(ge.status).toBe(400);
    expect(ge.code).toBe('network_not_found');
    expect(ge.message).toBe('unknown network');
    expect(ge.candidates).toEqual(['9536600adcf2b350']);
  });

  it('code 为 null 时保持 null 而非 undefined（daemon 错误体缺 error 字段）', () => {
    const ge = asError({ kind: 'api', status: 401, code: null, message: 'invalid token' });
    expect(ge.code).toBeNull();
    expect(ge.candidates).toBeUndefined();
  });

  it('Error 实例原样透传，name/message 保持可读且无结构化 status', () => {
    const err = new Error('boom');
    expect(asError(err)).toBe(err);
    expect(err.name).toBe('Error');
    expect(err.message).toBe('boom');
    expect(asError(err).status).toBeUndefined();
  });

  it('字符串 reject 值原样返回，读取结构化字段为 undefined', () => {
    expect(asError('plain failure')).toBe('plain failure');
    expect((asError('plain failure') as unknown as { status?: number }).status).toBeUndefined();
  });

  it('undefined reject 值原样返回', () => {
    expect(asError(undefined)).toBeUndefined();
  });
});
