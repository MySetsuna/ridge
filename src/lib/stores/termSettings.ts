import { writable } from 'svelte/store';

const FONT_SIZE_KEY = 'wind-term-font-size';
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
  const { subscribe, update } = writable(load());

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
      update(() => DEFAULT_FONT_SIZE);
    },
  };
}

/** Shared font-size for all terminal panes. Persisted in localStorage.
 *  Ctrl+= increases, Ctrl+- decreases, Ctrl+0 resets to 15. */
export const termFontSize = createTermFontSizeStore();
