/**
 * 语言（locale）状态：检测 → 持久化 → 切换。
 *
 * 设计要点：
 * - 仅两档：'zh'（中文）/ 'en'（英文/海外）。"中文 vs 外文(英文等)" 的二元划分。
 * - 结算地区由 locale 派生（见 billingRegion）：zh → 面包多卡密，en → 海外信用卡。
 *   语言即决定付费方案展示，用户不再单独切换结算地区。
 * - Tauri webview 同样暴露 navigator / Intl / localStorage，可直接复用。
 */
import { writable, derived, get } from 'svelte/store';

export type Locale = 'zh' | 'en';
export const LOCALES: readonly Locale[] = ['zh', 'en'];

/** 结算地区：cn=面包多卡密，intl=海外信用卡。完全由 locale 派生。 */
export type Region = 'cn' | 'intl';

const STORAGE_KEY = 'ridge.locale';

function isLocale(v: unknown): v is Locale {
  return v === 'zh' || v === 'en';
}

/** 检测初始语言：localStorage 覆盖 → navigator 语言 → 默认中文。 */
export function detectLocale(): Locale {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (isLocale(saved)) return saved;
  } catch {
    /* localStorage 不可用，忽略 */
  }
  try {
    const langs = [navigator.language, ...(navigator.languages ?? [])]
      .filter(Boolean)
      .map((l) => l.toLowerCase());
    if (langs.some((l) => l.startsWith('zh'))) return 'zh';
    if (langs.length > 0) return 'en';
  } catch {
    /* navigator 不可用，忽略 */
  }
  return 'zh';
}

export const locale = writable<Locale>(detectLocale());

// 持久化 + 同步 <html lang>。
locale.subscribe((l) => {
  try {
    localStorage.setItem(STORAGE_KEY, l);
  } catch {
    /* 忽略写入失败 */
  }
  try {
    document.documentElement.lang = l === 'zh' ? 'zh-CN' : 'en';
  } catch {
    /* 非浏览器环境，忽略 */
  }
});

/** 切换语言（同时驱动付费方案展示）。 */
export function setLocale(next: Locale): void {
  if (!isLocale(next)) return;
  if (get(locale) === next) return;
  locale.set(next);
}

/** 结算地区：zh → cn（面包多），en → intl（海外）。 */
export const billingRegion = derived(locale, ($l): Region => ($l === 'zh' ? 'cn' : 'intl'));
