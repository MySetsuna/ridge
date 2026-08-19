import { writable, get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { getTheme, setActiveBgImage } from './themes';

export interface UserSettings {
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
  // P4.4 (2026-05-21) — removed `parserBackend: 'wasm' | 'rust'` toggle.
  // The Rust-side PaneParser is the only parser path; `set_pane_delta_mode`
  // enables the native per-pane reader lane during activation and remains
  // available for the R5 self-heal force-reframe in ptyBridge.
  /// 2026-05-21 �?terminal IME helper textarea gate.
  /// 'ime': click �?focus invisible IME helper textarea so OS IME
  ///   composition events (CJK input methods) can attach. Each
  ///   keystroke routes through compositionstart/update/end. Default
  ///   for users who type Chinese / Japanese / Korean into shells.
  /// 'direct': skip the IME helper, container takes keydown directly.
  ///   ASCII characters go straight to PTY with no composition
  ///   buffering. Pick this when you only type English to shells and
  ///   don't want a half-toggled CJK IME to swallow plain ASCII
  ///   keystrokes (the "history input flickers with cursor" symptom).
  terminalImeMode: 'ime' | 'direct';
  /// Remote control server enabled on last session. Restored on boot to
  /// automatically start the remote server if the user left it on.
  remoteEnabled: boolean;
  /// 智能体协同（Domain Zero）总开关。**仅控制 UI 露出**（指挥部 Tab / pane
  /// 「设为智能体」入口）；不影响安全闸。默认开（仅呈现指挥部空态，零打扰）。
  teammateEnabled: boolean;
  /// 安全审批网关（HITL）。开后 L2 危险命令弹审批模态。默认关 —— 与后端
  /// `set_hitl_enabled` 默认一致，保证 send-keys 行为零变化。**独立生效，不被总
  /// 开关左右**（不可整体关：开启的安全闸不会被无关 UI 开关静默撤销）。
  teammateHitlEnabled: boolean;
}

const DEFAULTS: UserSettings = {
  theme: 'endless-dark',
  editorFontSize: 14,
  editorFontFamily: '',
  searchIncludeGlobs: '',
  searchExcludeGlobs: '',
  defaultShell: '',
  terminalFontFamily: '',
  defaultCwd: '',
  terminalPaddingPx: 0,
  terminalScrollbackLines: 2000,
  terminalImeMode: 'ime',
  remoteEnabled: false,
  teammateEnabled: true,
  teammateHitlEnabled: false,
};

const LS_KEY = 'ridge-settings';

function stringSetting(obj: Record<string, unknown>, key: string, fallback: string, required = false): string {
  const value = obj[key];
  return typeof value === 'string' && (!required || value.length > 0) ? value : fallback;
}

function numberSetting(
  obj: Record<string, unknown>,
  key: string,
  fallback: number,
  min: number,
  max: number,
  round = false,
): number {
  const value = obj[key];
  if (typeof value !== 'number' || !Number.isFinite(value) || value < min || value > max) return fallback;
  return round ? Math.round(value) : value;
}

function choiceSetting<T extends string>(obj: Record<string, unknown>, key: string, options: readonly T[], fallback: T): T {
  const value = obj[key];
  return typeof value === 'string' && options.includes(value as T) ? value as T : fallback;
}

function booleanSetting(obj: Record<string, unknown>, key: string, fallback: boolean): boolean {
  const value = obj[key];
  return typeof value === 'boolean' ? value : fallback;
}

function parseSettings(obj: Record<string, unknown>): UserSettings {
  return {
    theme: stringSetting(obj, 'theme', DEFAULTS.theme, true),
    editorFontSize: numberSetting(obj, 'editorFontSize', DEFAULTS.editorFontSize, 8, 32),
    editorFontFamily: stringSetting(obj, 'editorFontFamily', DEFAULTS.editorFontFamily),
    searchIncludeGlobs: stringSetting(obj, 'searchIncludeGlobs', DEFAULTS.searchIncludeGlobs),
    searchExcludeGlobs: stringSetting(obj, 'searchExcludeGlobs', DEFAULTS.searchExcludeGlobs),
    defaultShell: stringSetting(obj, 'defaultShell', DEFAULTS.defaultShell),
    terminalFontFamily: stringSetting(obj, 'terminalFontFamily', DEFAULTS.terminalFontFamily),
    defaultCwd: stringSetting(obj, 'defaultCwd', DEFAULTS.defaultCwd),
    terminalPaddingPx: numberSetting(obj, 'terminalPaddingPx', DEFAULTS.terminalPaddingPx, 0, 64),
    terminalScrollbackLines: numberSetting(obj, 'terminalScrollbackLines', DEFAULTS.terminalScrollbackLines, 100, 10000, true),
    terminalImeMode: choiceSetting(obj, 'terminalImeMode', ['ime', 'direct'] as const, DEFAULTS.terminalImeMode),
    remoteEnabled: booleanSetting(obj, 'remoteEnabled', DEFAULTS.remoteEnabled),
    teammateEnabled: booleanSetting(obj, 'teammateEnabled', DEFAULTS.teammateEnabled),
    teammateHitlEnabled: booleanSetting(obj, 'teammateHitlEnabled', DEFAULTS.teammateHitlEnabled),
  };
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
  try {
    const parsed: unknown = JSON.parse(raw);
    return parsed && typeof parsed === 'object'
      ? parseSettings(parsed as Record<string, unknown>)
      : { ...DEFAULTS };
  } catch {
    return { ...DEFAULTS };
  }
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

export function applyTheme(themeId: string): void {
  if (typeof document === 'undefined') return;
  const t = getTheme(themeId);
  if (!t) return;
  for (const [key, value] of Object.entries(t.colors)) {
    document.documentElement.style.setProperty(`--rg-${key}`, value);
  }
  // 解析该主题的终端背景图（自定义主题专属，异步、fire-and-forget）。
  void setActiveBgImage(themeId);
}

export function setTheme(themeId: string): void {
  applyTheme(themeId);
  setSetting('theme', themeId);
  // §theme-isolation: a remote control end (desktop-in-browser web-remote) must
  // NOT write its theme back to the host. `set_active_theme` mutates the HOST's
  // active theme + persists it to disk, and the host re-pushes that theme to
  // EVERY connected control end (mobile, other web-remotes) — so a remote
  // session's theme pick would clobber the host and every peer (the missing
  // isolation). The host write is only meaningful on the native desktop, where
  // this app IS the host; in web-remote the theme stays local (CSS vars +
  // localStorage + kernel via the theme bridge), isolated per control end.
  if (import.meta.env.RIDGE_WEB_REMOTE === true) return;
  // Persist to disk so the next launch's splash can render with the correct
  // loader colors BEFORE any JS has run. Fire-and-forget — failure only affects
  // the next-launch splash, never the current session.
  invoke('set_active_theme', { themeId }).catch((e) => {
    console.warn('[settings] set_active_theme persistence failed', e);
  });
}

export function initSettingsBoot(): void {
  const s = get(store);
  applyTheme(s.theme);
  // Restore remote control server state if it was enabled on last session.
  if (s.remoteEnabled) {
    invoke('set_remote_enabled', { enabled: true }).catch((e) => {
      console.warn('[settings] remote server auto-start failed', e);
    });
  }
}
