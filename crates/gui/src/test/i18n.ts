import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import en from '../i18n/en';
import zh from '../i18n/zh';

/// 测试专用 i18n 初始化：挂真实 zh/en 资源。
/// 不 import src/i18n/index.ts——它会在模块加载时读取 localStorage 并 invoke('set_lang')。
/// 注意 src/i18n/index.ts 是全局单例，App.test.tsx 引入 App 时仍会触发一次它的 init；
/// i18next 允许重复 init，此处最后执行的 init/changeLanguage 决定最终语言。
export async function initTestI18n(lng: 'zh' | 'en' = 'zh'): Promise<typeof i18n> {
  await i18n.use(initReactI18next).init({
    resources: { zh: { translation: zh }, en: { translation: en } },
    lng,
    fallbackLng: 'zh',
    interpolation: { escapeValue: false },
  });
  await i18n.changeLanguage(lng);
  return i18n;
}
