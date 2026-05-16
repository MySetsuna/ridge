import { writable, get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';

export interface LoaderColors {
  primary: string;
  secondary: string;
}

export interface ThemeEntry {
  id: string;
  label: string;
  type: 'dark' | 'light';
  loader: LoaderColors;
  colors: Record<string, string>;
}

export interface ThemeFile {
  version: number;
  themes: ThemeEntry[];
}

const CACHE_KEY = 'ridge-theme-data';

const store = writable<ThemeFile>({ version: 1, themes: [] });

export const themeData = { subscribe: store.subscribe };

export function getThemeIds(): string[] {
  return get(store).themes.map(t => t.id);
}

export function getThemeLabels(): Record<string, string> {
  const out: Record<string, string> = {};
  for (const t of get(store).themes) {
    out[t.id] = t.label;
  }
  return out;
}

export function getTheme(id: string): ThemeEntry | undefined {
  return get(store).themes.find(t => t.id === id);
}

export function isDarkTheme(id: string): boolean {
  const t = get(store).themes.find(x => x.id === id);
  return t?.type === 'dark';
}

function loadCache(): ThemeFile | null {
  if (typeof localStorage === 'undefined') return null;
  try {
    const raw = localStorage.getItem(CACHE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as ThemeFile;
    if (parsed.version >= 1 && parsed.themes.length > 0) return parsed;
  } catch { /* ignore */ }
  return null;
}

function saveCache(tf: ThemeFile): void {
  if (typeof localStorage === 'undefined') return;
  try {
    localStorage.setItem(CACHE_KEY, JSON.stringify(tf));
  } catch { /* quota */ }
}

let _resolved = false;

export function isThemeSystemResolved(): boolean {
  return _resolved;
}

export async function initThemeSystem(): Promise<void> {
  if (_resolved) return;

  const cached = loadCache();
  if (cached) {
    store.set(cached);
    _resolved = true;
  }

  try {
    const tf = await invoke<ThemeFile>('get_theme_data');
    store.set(tf);
    saveCache(tf);
    _resolved = true;
  } catch (e) {
    console.warn('get_theme_data failed, using cached/fallback', e);
    if (!cached) {
      console.error('no theme data available');
    }
  }
}
