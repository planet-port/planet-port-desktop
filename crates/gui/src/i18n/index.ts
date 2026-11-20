import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import en from './en';
import zh from './zh';

const saved = localStorage.getItem('lang');
const initial = saved === 'en' || saved === 'zh' ? saved : navigator.language.toLowerCase().startsWith('zh') ? 'zh' : 'en';

void i18n.use(initReactI18next).init({
  resources: { zh: { translation: zh }, en: { translation: en } },
  lng: initial,
  fallbackLng: 'zh',
  interpolation: { escapeValue: false },
});

/// 初始语言同步托盘菜单,否则托盘始终为 Rust 侧默认文案、与 GUI 首屏语言不一致
void invoke('set_lang', { lang: initial });

/// 切换 GUI 语言并同步托盘菜单语言（Rust 侧状态）
export function switchLang(lang: 'zh' | 'en') {
  localStorage.setItem('lang', lang);
  void i18n.changeLanguage(lang);
  void invoke('set_lang', { lang });
}
