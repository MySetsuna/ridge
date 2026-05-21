import { writable, get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { getTheme } from './themes';

export type ParserBackend = 'wasm' | 'rust';

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
  /// P3.7 (2026-05-20) — which VT parser runs PTY bytes:
  /// 'rust': src-tauri/src/engine/parser.rs::PaneParser produces GridDelta
  ///   frames; wasm consumer applies them via `kernel.applyDeltaFrame`.
  ///   Main-thread CPU drops because the JS thread no longer runs vte.
  /// 'wasm': legacy path — JS thread calls `kernel.feed(bytes)` and the
  ///   wasm parser walks the state machine on the main thread.
  /// Default: 'rust'. Switching takes effect next pane attach (manager.ts
  ///   handles detach/reattach with a 200ms fade mask, see P3.9).
  parserBackend: ParserBackend;
  /// 2026-05-21 — terminal IME helper textarea gate.
  /// 'ime': click → focus invisible IME helper textarea so OS IME
  ///   composition events (CJK input methods) can attach. Each
  ///   keystroke routes through compositionstart/update/end. Default
  ///   for users who type Chinese / Japanese / Korean into shells.
  /// 'direct': skip the IME helper, container takes keydown directly.
  ///   ASCII characters go straight to PTY with no composition
  ///   buffering. Pick this when you only type English to shells and
  ///   don't want a half-toggled CJK IME to swallow plain ASCII
  ///   keystrokes (the "history input flickers with cursor" symptom).
  terminalImeMode: 'ime' | 'direct';
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
  parserBackend: 'rust',
  terminalImeMode: 'ime',
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
    parserBackend:
      obj.parserBackend === 'wasm' || obj.parserBackend === 'rust'
        ? obj.parserBackend
        : DEFAULTS.parserBackend,
    terminalImeMode:
      obj.terminalImeMode === 'ime' || obj.terminalImeMode === 'direct'
        ? obj.terminalImeMode
        : DEFAULTS.terminalImeMode,
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
