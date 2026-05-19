import { writable, get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { getTheme } from './themes';

export interface UserSettings {
  claudeExtensionEnabled: boolean;
  theme: string;
  editorFontSize: number;
  editorFontFamily: string;
  searchIncludeGlobs: string;
  searchExcludeGlobs: string;
  defaultShell: string;
  terminalFontFamily: string;
  defaultCwd: string;
  terminalPaddingPx: number;
  terminalScrollbackLines: number;
}

const DEFAULTS: UserSettings = {
  claudeExtensionEnabled: true,
  theme: 'endless-dark',
  editorFontSize: 14,
  editorFontFamily: '',
  searchIncludeGlobs: '',
  searchExcludeGlobs: '',
  defaultShell: '',
  terminalFontFamily: '',
  defaultCwd: '',
  terminalPaddingPx: 6,
  terminalScrollbackLines: 2000,
};

const LS_KEY = 'ridge-settings';

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
    theme:
      typeof obj.theme === 'string' && obj.theme.length > 0
        ? obj.theme
        : DEFAULTS.theme,
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
    document.cookie = `ridge-theme=${s.theme}; path=/; max-age=31536000; SameSite=Lax`;
  } catch { /* quota */ }
}

const store = writable<UserSettings>(load());

export const settingsStore = { subscribe: store.subscribe };

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

export function applyTheme(themeId: string): void {
  if (typeof document === 'undefined') return;
  const t = getTheme(themeId);
  if (!t) return;
  for (const [key, value] of Object.entries(t.colors)) {
    document.documentElement.style.setProperty(`--rg-${key}`, value);
  }
}

export function setTheme(themeId: string): void {
  applyTheme(themeId);
  setSetting('theme', themeId);
  // Persist to disk so the next launch's splash can render with the
  // correct loader colors BEFORE any JS has run. Without this the
  // first-frame splash would always fall back to the bootstrap theme.
  // Fire-and-forget — failure to persist only affects the next-launch
  // splash, never the current session.
  invoke('set_active_theme', { themeId }).catch((e) => {
    console.warn('[settings] set_active_theme persistence failed', e);
  });
}

export function initSettingsBoot(): void {
  applyTheme(get(store).theme);
}
