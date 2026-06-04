/**
 * i18n 入口：t（响应式 markup 用）、tr（同步事件/异步逻辑用）、翻译核心。
 *
 *   markup：  {$t('cloudPro.title')}
 *   脚本：    toast(tr('settings.saved'))
 *   插值：    $t('remote.connectedAs', { name })
 */
import { derived, get } from 'svelte/store';
import { locale, type Locale } from './locale';
import { messages } from './messages';

export { locale, setLocale, billingRegion, detectLocale, LOCALES } from './locale';
export type { Locale, Region } from './locale';

export type TranslateVars = Record<string, string | number>;

function lookup(dict: unknown, key: string): unknown {
  return key.split('.').reduce<unknown>((node, seg) => {
    if (node && typeof node === 'object') return (node as Record<string, unknown>)[seg];
    return undefined;
  }, dict);
}

function interpolate(template: string, vars?: TranslateVars): string {
  if (!vars) return template;
  return template.replace(/\{(\w+)\}/g, (_, name: string) =>
    vars[name] != null ? String(vars[name]) : `{${name}}`
  );
}

/** 纯函数翻译：缺失键回退到 zh，再回退到 key 本身。 */
export function translate(loc: Locale, key: string, vars?: TranslateVars): string {
  const raw = lookup(messages[loc], key) ?? lookup(messages.zh, key);
  if (typeof raw !== 'string') return key;
  return interpolate(raw, vars);
}

/** 响应式翻译函数 store：locale 变化时自动重算 markup。 */
export const t = derived(
  locale,
  ($l) =>
    (key: string, vars?: TranslateVars): string =>
      translate($l, key, vars)
);

/** 同步翻译（事件处理 / 异步逻辑 / 非响应式上下文）。 */
export function tr(key: string, vars?: TranslateVars): string {
  return translate(get(locale), key, vars);
}
