import { writable, get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';

// Splash loader contract. `primary` / `secondary` are required and feed
// the SVG stroke and accent fill. Everything else is optional — fields
// missing from a theme fall back to the hardcoded defaults baked into
// `src/app.html`'s CSS variables (no per-theme value = current visual).
//
// Numbers are interpreted as: pixel lengths for *Width / *Radius,
// milliseconds for *DurationMs / *DelayMs, raw scalars for opacities and
// the breathe-scale knob.
export interface LoaderConfig {
  primary: string;
  secondary: string;
  bg?: string;
  accentGlow?: string;
  strokeWidth?: number;
  cornerRadius?: number;
  drawDurationMs?: number;
  breatheDurationMs?: number;
  crossDelayMs?: number;
  fadeOutDurationMs?: number;
  fillOpacityPrimary?: number;
  fillOpacitySecondary?: number;
}

export interface ThemeEntry {
  id: string;
  label: string;
  type: 'dark' | 'light';
  loader: LoaderConfig;
  colors: Record<string, string>;
}

export interface ThemeFile {
  version: number;
  themes: ThemeEntry[];
}

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

let _resolved = false;

export async function initThemeSystem(): Promise<void> {
  if (_resolved) return;
  try {
    const tf = await invoke<ThemeFile>('get_theme_data');
    store.set(tf);
    _resolved = true;
  } catch (e) {
    console.error('initThemeSystem failed', e);
    throw e;
  }
}
