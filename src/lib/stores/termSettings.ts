import { writable } from 'svelte/store';

const FONT_SIZE_KEY = 'ridge-term-font-size';
const DEFAULT_FONT_SIZE = 15;
const MIN_FONT_SIZE = 8;
const MAX_FONT_SIZE = 32;

function load(): number {
  if (typeof localStorage === 'undefined') return DEFAULT_FONT_SIZE;
  const raw = localStorage.getItem(FONT_SIZE_KEY);
  if (!raw) return DEFAULT_FONT_SIZE;
  const n = parseInt(raw, 10);
  return isNaN(n) ? DEFAULT_FONT_SIZE : Math.max(MIN_FONT_SIZE, Math.min(MAX_FONT_SIZE, n));
}

function persist(size: number): void {
  if (typeof localStorage !== 'undefined') {
    localStorage.setItem(FONT_SIZE_KEY, String(size));
  }
}

function createTermFontSizeStore() {
  const { subscribe, update, set } = writable(load());

  return {
    subscribe,
    increase() {
      update((s) => {
        const next = Math.min(MAX_FONT_SIZE, s + 1);
        persist(next);
        return next;
      });
    },
    decrease() {
      update((s) => {
        const next = Math.max(MIN_FONT_SIZE, s - 1);
        persist(next);
        return next;
      });
    },
    reset() {
      persist(DEFAULT_FONT_SIZE);
      set(DEFAULT_FONT_SIZE);
    },
    /** 直接设到指定字号；用于设置面板的 slider。NaN / 超界自动 clamp。 */
    setSize(value: number) {
      const clamped = Math.max(
        MIN_FONT_SIZE,
        Math.min(MAX_FONT_SIZE, Number.isFinite(value) ? Math.round(value) : DEFAULT_FONT_SIZE)
      );
      persist(clamped);
      set(clamped);
    },
  };
}

/** Shared font-size for all terminal panes. Persisted in localStorage.
 *  Ctrl+= increases, Ctrl+- decreases, Ctrl+0 resets to 15. */
export const termFontSize = createTermFontSizeStore();

/** Helper for callers that don't want to import the store object directly
 *  （例如 SettingsPanel 的 slider）。 */
export function setTermFontSize(value: number): void {
  termFontSize.setSize(value);
}
