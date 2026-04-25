// src/lib/stores/settings.ts
//
// User-toggleable preferences. Today this only carries the "Claude Code"
// extension enable flag, but the shape is intentionally generic so future
// features (telemetry, font size, theme, plugin allowlist) can land here
// without inventing a new store each time.
//
// Persistence: localStorage with a single JSON blob — small payload, atomic
// read/write, no Tauri/IPC dependency. SSR-safe (every accessor checks
// `typeof localStorage`).

import { writable } from 'svelte/store';

export interface UserSettings {
  /**
   * When true, the Claude Code extension surface is mounted: 4th rail
   * button, dedicated sidebar tab, per-pane Bot launcher button. When
   * false, none of these render — the rest of the app behaves as if the
   * extension didn't exist. Defaults to true so existing users see no
   * regression on first launch after upgrade.
   */
  claudeExtensionEnabled: boolean;
}

const DEFAULTS: UserSettings = {
  claudeExtensionEnabled: true,
};

const LS_KEY = 'wind-settings';

/**
 * Type-narrow each known setting key independently — a tampered or
 * hand-edited blob with `claudeExtensionEnabled: "yes"` would previously
 * spread a string over a boolean (TS cast gave false safety). Now any
 * non-matching value silently falls back to the default for that key
 * while neighbouring valid keys still load. Avoids a hard zod dep for a
 * 1-key store; revisit when `UserSettings` grows past 3-4 fields.
 */
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

/** Convenience for the hot path (extension toggle). */
export function setClaudeExtensionEnabled(enabled: boolean): void {
  setSetting('claudeExtensionEnabled', enabled);
}
