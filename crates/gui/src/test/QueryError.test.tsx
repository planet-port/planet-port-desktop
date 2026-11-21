import { render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it } from 'vitest';
import QueryError from '../components/QueryError';
import { initTestI18n } from './i18n';

describe('QueryError', () => {
  beforeEach(async () => {
    await initTestI18n('zh');
  });

  it('status=401 走专用鉴权失败文案（避免误导用户以为数据丢失）', () => {
    render(<QueryError error={{ kind: 'api', status: 401, code: null, message: 'invalid token' }} />);
    expect(
      screen.getByText('鉴权失败：token 与 daemon 不匹配，请检查 PLANET_PORT_TOKEN 或重装服务'),
    ).toBeInTheDocument();
    expect(screen.queryByText(/加载失败/)).not.toBeInTheDocument();
  });

  it('非 401 走通用加载失败文案并附错误 detail', () => {
    render(<QueryError error={{ kind: 'offline', message: 'daemon unreachable at http://127.0.0.1:9998' }} />);
    expect(screen.getByText('加载失败（daemon unreachable at http://127.0.0.1:9998）')).toBeInTheDocument();
  });

  it('reject 值缺 message（如 undefined）时仅显示通用文案', () => {
    render(<QueryError error={undefined} />);
    expect(screen.getByText('加载失败')).toBeInTheDocument();
  });
});
