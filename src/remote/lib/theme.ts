// Apply the desktop's active theme (pushed over WS as a `ridge.theme` colors
// map) to the remote page: CSS custom properties for the chrome, plus an
// xterm.js-shaped palette for the wasm terminal kernel.
//
// The colors map keys are the `--rg-*` variable names without the prefix
// (bg, surface, accent, ansi-red, …). Chrome styles read `var(--rg-*)`; the
// kernel needs hex, so the kernel palette is normalized via `hex8` — the same
// path the desktop's themeBridge uses.

import { hex8 } from '@ridge/remote/shared/terminal/cssColor';

/**
 * Pick the colour the browser should use for the page edge/status bar.
 * `bg` is the chrome background; old/partial theme payloads may only carry
 * `term-bg`, so keep that as a safe fallback.
 */
export function themeChromeColor(colors: Record<string, string>): string | null {
  for (const key of ['bg', 'term-bg']) {
    const value = colors[key];
    if (typeof value === 'string' && value.trim().length > 0) return value.trim();
  }
  return null;
}

/**
 * Apply a theme in one browser style commit.
 *
 * Updating dozens of custom properties one-by-one lets a mobile WebView paint
 * an intermediate frame (the page edge still uses the old `theme-color` while
 * the content already uses the new `--rg-bg`). Build the declaration on a
 * detached style object, then replace the root style once; sync the body and
 * browser chrome colour in the same task. This keeps PWA edge pixels and the
 * document background on the same palette without changing terminal behaviour.
 */
export function applyThemeVars(colors: Record<string, string>): void {
  if (typeof document === 'undefined') return;
  const root = document.documentElement;
  const entries = Object.entries(colors)
    .filter(([key, value]) => key.length > 0 && typeof value === 'string');
  const chromeColor = themeChromeColor(colors);

  // Prepare the complete declaration off-DOM. Preserve unrelated inline
  // startup/runtime variables and only replace the live style attribute once.
  let atomicApplied = false;
  try {
    const draft = document.createElement('div');
    draft.style.cssText = root.style.cssText;
    for (const [key, value] of entries) draft.style.setProperty(`--rg-${key}`, value);
    if (chromeColor) draft.style.backgroundColor = chromeColor;
    root.style.cssText = draft.style.cssText;
    atomicApplied = true;
  } catch {
    // Very small DOM shims (SSR/tests) may not expose createElement/style.cssText.
    // Keep the old behaviour as a defensive fallback.
    for (const [key, value] of entries) root.style.setProperty(`--rg-${key}`, value);
  }

  if (chromeColor) {
    // Explicit edge fills avoid a one-frame transparent strip while custom
    // properties recascade on mobile Safari/Chromium standalone windows.
    // `background-color` was included in the atomic declaration above. The
    // fallback path (or a minimal style shim) still needs the direct write.
    if (!atomicApplied) root.style.backgroundColor = chromeColor;
    if (document.body) document.body.style.backgroundColor = chromeColor;

    const meta = document.querySelector<HTMLMetaElement>('meta[name="theme-color"]');
    if (meta) meta.content = chromeColor;
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
