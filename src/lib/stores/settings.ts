// src/lib/stores/settings.ts
//
// Centralised user-toggleable preferences. Backed by a single `ridge-settings`
// JSON blob in localStorage; one writer / one reader path keeps it atomic.
// Per-key type narrowing in `load()` lets neighbouring valid keys survive
// when one is tampered or comes from an older schema.

import { writable, get } from 'svelte/store';

/** Theme id. Each id maps to a `[data-rg-theme="<id>"]` block in `app.css`.
 *  'dark' uses the :root defaults (森林深色). */
export type ThemeId = 'dark' | 'sand' | 'grass' | 'soil' | 'wheat' | 'starsky';

export const THEME_IDS: ThemeId[] = ['dark', 'sand', 'grass', 'soil', 'wheat', 'starsky'];

/** Display label for the theme switcher. */
export const THEME_LABELS: Record<ThemeId, string> = {
  dark:    '森林深色',
  sand:    '沙土浅色',
  grass:   '草地浅色',
  soil:    '土壤深色',
  wheat:   '麦田浅色',
  starsky: '星空深色',
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
  /**
   * 终端字体族，逗号分隔。空串表示走默认 fallback chain。
   */
  terminalFontFamily: string;
  /**
   * 终端初始工作目录绝对路径。空串 = 跟随系统默认（通常是用户 home 目录）。
   */
  defaultCwd: string;
  /**
   * 终端 pane 内边距（px），把 wasm 渲染的 canvas 从 pane 边框向内推。
   * 0 = 紧贴边框（早期默认）。建议 4-12 之间，避免字符被滚动条 / 分隔线擦边。
   */
  terminalPaddingPx: number;
  /**
   * 终端单 pane scrollback 行数。新建 pane 时生效；已开 pane 需要重启
   * 该 pane 才会切换容量（kernel 端 ring buffer 在构造期固定大小）。
   *
   * 范围 100..=10000。默认 2000，约 ≈ 5 MiB / pane（80 cols × 32 B/cell ×
   * 2000 行）。10000 行约 ≈ 25 MiB / pane — 仍在桌面级内存预算之内但
   * 已是用户应自觉权衡的上限；100 是最小值（覆盖一屏即可的轻量模式）。
   *
   * 用户反馈「scrollback 太大」时应往下调；反馈「pageup 翻不到」时往
   * 上调。配合 §B.2 ED 3 物理清理（右键「清空」），运行期可以随时
   * 把已积累的 scrollback 清掉而无需重启 pane。
   */
  terminalScrollbackLines: number;
}

const DEFAULTS: UserSettings = {
  claudeExtensionEnabled: true,
  theme: 'dark',
  editorFontSize: 14,
  editorFontFamily: '',
  searchIncludeGlobs: '',
  searchExcludeGlobs: '',
  defaultShell: '',
  terminalFontFamily: '"JetBrains Mono", "Cascadia Code", "SF Mono", ui-monospace, Consolas, "SimHei", "Heiti SC", "Microsoft YaHei", "Apple Color Emoji", "Segoe UI Emoji", "Noto Color Emoji", monospace',
  defaultCwd: '',
  terminalPaddingPx: 6,
  terminalScrollbackLines: 2000,
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
    terminalFontFamily:
      typeof obj.terminalFontFamily === 'string' ? obj.terminalFontFamily : DEFAULTS.terminalFontFamily,
    defaultCwd:
      typeof obj.defaultCwd === 'string' ? obj.defaultCwd : DEFAULTS.defaultCwd,
    terminalPaddingPx:
      typeof obj.terminalPaddingPx === 'number' &&
      Number.isFinite(obj.terminalPaddingPx) &&
      obj.terminalPaddingPx >= 0 &&
      obj.terminalPaddingPx <= 64
        ? obj.terminalPaddingPx
        : DEFAULTS.terminalPaddingPx,
    terminalScrollbackLines:
      typeof obj.terminalScrollbackLines === 'number' &&
      Number.isFinite(obj.terminalScrollbackLines) &&
      obj.terminalScrollbackLines >= 100 &&
      obj.terminalScrollbackLines <= 10000
        ? Math.round(obj.terminalScrollbackLines)
        : DEFAULTS.terminalScrollbackLines,
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
  applyTheme(theme);
  setSetting('theme', theme);
}

/** 启动初始化：从 store 读出当前主题并写到 root，使 SSR 后首帧不闪 default。 */
export function initSettingsBoot(): void {
  applyTheme(get(store).theme);
}
