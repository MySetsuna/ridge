/**
 * 结算地区推断（方案 1）。
 *
 * 仅用于决定升级弹窗里支付方式的**默认**项；用户始终可手动切换。
 * - 简体中文 或 中国大陆时区  → 'cn'  （面包多卡密）
 * - 繁体中文（台/港/澳）/ 其它 → 'intl'（Lemon Squeezy 信用卡）
 *
 * Tauri webview 同样暴露 navigator.language / Intl，直接复用浏览器探测。
 */

export type Region = 'cn' | 'intl';

// 中国大陆常见 IANA 时区标识（含历史别名）。
const CN_TIMEZONES = new Set([
  'Asia/Shanghai',
  'Asia/Chongqing',
  'Asia/Harbin',
  'Asia/Urumqi',
  'Asia/Kashgar',
  'PRC'
]);

/** 根据浏览器语言与时区猜测用户更可能使用的结算地区。 */
export function detectPreferredRegion(): Region {
  try {
    const langs = [navigator.language, ...(navigator.languages ?? [])]
      .filter(Boolean)
      .map((l) => l.toLowerCase());

    // 繁体中文（台/港/澳）信用卡更普遍，归为 intl。
    const isTraditional = langs.some(
      (l) =>
        l.startsWith('zh-tw') ||
        l.startsWith('zh-hk') ||
        l.startsWith('zh-mo') ||
        l.startsWith('zh-hant')
    );
    // 简体中文 → cn。
    const isSimplified = langs.some(
      (l) => l === 'zh' || l.startsWith('zh-cn') || l.startsWith('zh-hans')
    );

    let tz = '';
    try {
      tz = Intl.DateTimeFormat().resolvedOptions().timeZone ?? '';
    } catch {
      /* 某些环境不支持，忽略 */
    }
    const isCnTimezone = CN_TIMEZONES.has(tz);

    if (isSimplified) return 'cn';
    if (isTraditional) return 'intl';
    if (isCnTimezone) return 'cn';
    return 'intl';
  } catch {
    // 检测失败时默认国内（主力用户群）。
    return 'cn';
  }
}
