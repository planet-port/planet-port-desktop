import { useTranslation } from 'react-i18next';
import { asError } from '../api/client';

/// 查询失败提示块:与空态区分显示。401(token 失配)给专用文案,避免误导用户以为数据丢失。
export default function QueryError({ error }: { error: unknown }) {
  const { t } = useTranslation();
  const ge = asError(error);
  if (ge?.status === 401) return <div className="empty">{t('common.authError')}</div>;
  const detail = ge?.message ? `（${ge.message}）` : '';
  return <div className="empty">{t('common.loadError')}{detail}</div>;
}
