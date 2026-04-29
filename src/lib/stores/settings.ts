// src/lib/stores/settings.ts
//
// Centralised user-toggleable preferences. Backed by a single `ridge-settings`
// JSON blob in localStorage; one writer / one reader path keeps it atomic.
// Per-key type narrowing in `load()` lets neighbouring valid keys survive
// when one is tampered or comes from an older schema.

import { writable, get } from 'svelte/store';

/** Theme id. Each id maps to a `[data-rg-theme="<id>"]` block in `app.css`. */
export type ThemeId = 'dark' | 'sand' | 'grass' | 'soil';

export const THEME_IDS: ThemeId[] = ['dark', 'sand', 'grass', 'soil'];

/** Display label for the theme switcher. Kept in this module so the panel
 *  doesn't need to duplicate the list. */
export const THEME_LABELS: Record<ThemeId, string> = {
  dark: '默认深色',
  sand: '沙土浅色',
  grass: '草地浅色',
  soil: '土壤深色',
};

export interface UserSettings {
  /** Claude Code extension surface (rail button + sidebar tab + Bot launcher). */
  claudeExtensionEnabled: boolean;
  /** UI 主题 id；驱动 `<html data-rg-theme>` 切换 CSS 变量集合。 */
  theme: ThemeId;
  /** Monaco 编辑器字号（px）。 */
  editorFontSize: number;
  /**
   * Monaco 编辑器字体族，逗号分隔。空串表示走默认 fallback chain
   *（与硬编码值一致）。仅当用户主动改写时才会非空。
   */
  editorFontFamily: string;
  /**
   * SearchSidebar 默认 include globs（逗号 / 换行分隔）。
   * 持久化避免用户每次重开都要重输 `**​/*.ts` 一类常用过滤。
   */
  searchIncludeGlobs: string;
  /** SearchSidebar 默认 exclude globs（默认 `node_modules` 等已在后端忽略，这里覆盖额外规则）。 */
  searchExcludeGlobs: string;
  /**
   * 默认 shell 程序绝对路径。空串 = 跟随系统默认（Windows: powershell.exe，
   * 其他: zsh）。`detect_available_shells` 返回的 program 字段可作为此字段值。
   */
  defaultShell: string;
}

const DEFAULTS: UserSettings = {
  claudeExtensionEnabled: true,
  theme: 'dark',
  editorFontSize: 13,
  editorFontFamily: '',
  searchIncludeGlobs: '',
  searchExcludeGlobs: '',
  defaultShell: '',
};

const LS_KEY = 'ridge-settings';

function isThemeId(v: unknown): v is ThemeId {
  return typeof v === 'string' && (THEME_IDS as string[]).includes(v);
}

function load(): UserSettings {
  if (typeof localStorage === 'undefined') return { ...DEFAULTS };
  const raw = (() => {
    try {
      return localStorage.getItem(LS_KEY);
    } catch {
      return null;
    }
  })();
  if (!raw) return { ...DEFAULTS };
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return { ...DEFAULTS };
  }
  if (!parsed || typeof parsed !== 'object') return { ...DEFAULTS };
  const obj = parsed as Record<string, unknown>;
  return {
    claudeExtensionEnabled:
      typeof obj.claudeExtensionEnabled === 'boolean'
        ? obj.claudeExtensionEnabled
        : DEFAULTS.claudeExtensionEnabled,
    theme: isThemeId(obj.theme) ? obj.theme : DEFAULTS.theme,
    editorFontSize:
      typeof obj.editorFontSize === 'number' &&
      Number.isFinite(obj.editorFontSize) &&
      obj.editorFontSize >= 8 &&
      obj.editorFontSize <= 32
        ? obj.editorFontSize
        : DEFAULTS.editorFontSize,
    editorFontFamily:
      typeof obj.editorFontFamily === 'string'
        ? obj.editorFontFamily
        : DEFAULTS.editorFontFamily,
    searchIncludeGlobs:
      typeof obj.searchIncludeGlobs === 'string'
        ? obj.searchIncludeGlobs
        : DEFAULTS.searchIncludeGlobs,
    searchExcludeGlobs:
      typeof obj.searchExcludeGlobs === 'string'
        ? obj.searchExcludeGlobs
        : DEFAULTS.searchExcludeGlobs,
    defaultShell:
      typeof obj.defaultShell === 'string' ? obj.defaultShell : DEFAULTS.defaultShell,
  };
}

function persist(s: UserSettings): void {
  if (typeof localStorage === 'undefined') return;
  try {
    localStorage.setItem(LS_KEY, JSON.stringify(s));
  } catch {
    /* quota — settings are best-effort */
  }
}

const store = writable<UserSettings>(load());

export const settingsStore = { subscribe: store.subscribe };

/** Update one setting key. Triggers persist + reactive subscribers. */
export function setSetting<K extends keyof UserSettings>(
  key: K,
  value: UserSettings[K]
): void {
  store.update((s) => {
    const next = { ...s, [key]: value };
    persist(next);
    return next;
  });
}

export function setClaudeExtensionEnabled(enabled: boolean): void {
  setSetting('claudeExtensionEnabled', enabled);
}

/** 把当前主题 id 写到 `<html data-rg-theme>`，让 app.css 中的覆盖块生效。
 *  幂等；启动时与每次 setTheme 都调用。 */
export function applyTheme(theme: ThemeId): void {
  if (typeof document === 'undefined') return;
  document.documentElement.dataset.rgTheme = theme;
}

export function setTheme(theme: ThemeId): void {
  setSetting('theme', theme);
  applyTheme(theme);
}

/** 启动初始化：从 store 读出当前主题并写到 root，使 SSR 后首帧不闪 default。 */
export function initSettingsBoot(): void {
  applyTheme(get(store).theme);
}
