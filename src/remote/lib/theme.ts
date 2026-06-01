// Apply the desktop's active theme (pushed over WS as a `ridge.theme` colors
// map) to the remote page: CSS custom properties for the chrome, plus an
// xterm.js-shaped palette for the wasm terminal kernel.
//
// The colors map keys are the `--rg-*` variable names without the prefix
// (bg, surface, accent, ansi-red, …). Chrome styles read `var(--rg-*)`; the
// kernel needs hex, so the kernel palette is normalized via `hex8` — the same
// path the desktop's themeBridge uses.

import { hex8 } from '$lib/utils/cssColor';

/** Set every theme color as a `--rg-*` custom property on :root. */
export function applyThemeVars(colors: Record<string, string>): void {
  if (typeof document === 'undefined') return;
  const root = document.documentElement;
  for (const [key, value] of Object.entries(colors)) {
    root.style.setProperty(`--rg-${key}`, value);
  }
}

// ridge.theme `ansi-*` color key → xterm.js palette key.
const ANSI_KEYS: Record<string, string> = {
  'ansi-black': 'black', 'ansi-red': 'red', 'ansi-green': 'green', 'ansi-yellow': 'yellow',
  'ansi-blue': 'blue', 'ansi-magenta': 'magenta', 'ansi-cyan': 'cyan', 'ansi-white': 'white',
  'ansi-brightBlack': 'brightBlack', 'ansi-brightRed': 'brightRed',
  'ansi-brightGreen': 'brightGreen', 'ansi-brightYellow': 'brightYellow',
  'ansi-brightBlue': 'brightBlue', 'ansi-brightMagenta': 'brightMagenta',
  'ansi-brightCyan': 'brightCyan', 'ansi-brightWhite': 'brightWhite',
};

/**
 * Project the theme colors onto the xterm.js-shaped key set the wasm kernel's
 * `applyTheme` (`Theme::apply_partial`) reads. Mirrors the desktop
 * `themeBridge.readRidgeTheme()` so both ends paint the terminal identically.
 * Only normalized keys are included — partial themes are fine.
 */
export function buildKernelTheme(colors: Record<string, string>): Record<string, string> {
  const norm = (v?: string) => (v ? hex8(v) : null);
  const out: Record<string, string> = {};
  const bg = norm(colors['term-bg']) ?? norm(colors['bg']);
  const fg = norm(colors['fg']);
  const accent = norm(colors['accent']);
  const tuiBg = norm(colors['tui-bg']);
  if (bg) out.background = bg;
  if (fg) out.foreground = fg;
  if (tuiBg) out.tuiBackground = tuiBg;
  if (accent) {
    out.cursor = accent;
    if (bg) out.cursorAccent = bg;
    out.hyperlinkColor = accent;
    const sel = norm(colors['selection-bg']);
    if (sel) out.selectionBackground = sel;
    else if (accent.length === 9) out.selectionBackground = `${accent.slice(0, 7)}3d`;
  }
  for (const [key, xterm] of Object.entries(ANSI_KEYS)) {
    const c = norm(colors[key]);
    if (c) out[xterm] = c;
  }
  return out;
}
