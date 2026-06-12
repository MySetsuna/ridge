// Flag-emoji support detection + on-demand subset-face registration.
//
// Shared by BOTH surfaces — the desktop terminal (themeBridge `pushFont`) and
// the web-remote controller — so the "system emoji + on-demand flag subset"
// policy lives in one place and renders identically on every platform. The pure
// logic (probe + cache) is split from the browser glue so it stays unit-testable
// under the `node` vitest environment.

import { SYSTEM_EMOJI_FALLBACK } from './fontStack';

/** unicode-range-gated @font-face for the flag-only subset. Injected ONLY when
 *  the OS can't render flags; the browser then downloads /fonts/flags.woff2
 *  lazily — only when a flag codepoint actually appears. Note: the caller must
 *  also prepend 'Flag Emoji' to the target element's font-family stack — the
 *  @font-face declaration alone does not place the family into the cascade.
 *  `/fonts/flags.woff2` resolves on both surfaces: desktop serves it from
 *  static/fonts/, the web-remote from src/remote/public/fonts/. */
export const FLAG_FONT_FACE_CSS =
  "@font-face{font-family:'Flag Emoji';" +
  "src:url('/fonts/flags.woff2') format('woff2');" +
  'unicode-range:U+1F1E6-1F1FF,U+1F3F4,U+E0020-E007F;' +
  'font-display:swap;}';

/** localStorage key holding the cached probe verdict + its UA fingerprint. */
export const FLAG_CACHE_KEY = 'ridge.flagEmojiSupport';

/**
 * Decide whether the OS natively renders Regional-Indicator flags, from text
 * measurements. A supported OS merges a RI pair into ONE flag glyph (advance ≈
 * a single RI letter); an unsupported OS lays the pair out as TWO letter
 * glyphs (≈ double width). `measure` returns the advance width (px) of the
 * given string. Falls back to `true` (assume supported → inject nothing) when
 * measurement is unavailable, so we never ship a font we can't justify.
 */
export function probeSystemFlagSupport(measure: (text: string) => number): boolean {
  const single = measure('\u{1F1EF}'); // 🇯 one Regional Indicator
  const pair = measure('\u{1F1EF}\u{1F1F5}'); // 🇯🇵 Japan flag OR "JP"
  if (!(single > 0) || !(pair > 0)) return true; // can't measure either side → assume supported (also covers pair===0)
  return pair < single * 1.5; // merged into one glyph → supported
}

interface FlagCacheEntry {
  ua: string;
  supported: boolean;
}

/** Parse a cached verdict, honouring the UA fingerprint. Returns the cached
 *  `supported` boolean only when the fingerprint matches; otherwise null
 *  (absent/corrupt/stale → caller must re-probe). */
export function readFlagCache(raw: string | null, ua: string): boolean | null {
  if (!raw) return null;
  let parsed: Partial<FlagCacheEntry>;
  try {
    parsed = JSON.parse(raw) as Partial<FlagCacheEntry>;
  } catch {
    return null;
  }
  if (parsed.ua !== ua || typeof parsed.supported !== 'boolean') return null;
  return parsed.supported;
}

/** Serialize a verdict + UA fingerprint for persistence. */
export function writeFlagCache(supported: boolean, ua: string): string {
  return JSON.stringify({ ua, supported } satisfies FlagCacheEntry);
}

// ───────────────────────────── Browser glue ────────────────────────────────

/**
 * Resolve whether the terminal needs the flag subset face, using a cached
 * verdict when present else a one-shot canvas probe, and register the
 * @font-face when the OS lacks flags. Returns true when the OS lacks native
 * flags and the 'Flag Emoji' fallback face has therefore been injected (so the
 * caller should include 'Flag Emoji' in the font stack); false when the OS
 * renders flags natively or in a non-DOM context. Safe to call repeatedly
 * (cache + idempotent injection). Never throws; returns false in non-DOM
 * contexts.
 */
export function ensureFlagFont(): boolean {
  if (typeof document === 'undefined') return false;
  const ua = typeof navigator !== 'undefined' ? navigator.userAgent : '';
  let supported: boolean | null = null;
  try {
    supported = readFlagCache(localStorage.getItem(FLAG_CACHE_KEY), ua);
  } catch {
    supported = null; // private mode / disabled storage
  }
  if (supported === null) {
    supported = probeSystemFlagSupport(measureWithCanvas);
    try {
      localStorage.setItem(FLAG_CACHE_KEY, writeFlagCache(supported, ua));
    } catch {
      /* quota / private mode — proceed without caching */
    }
  }
  const needsFallback = !supported;
  if (needsFallback) injectFlagFontFace();
  return needsFallback;
}

/** Measure advance width of `text` under the system emoji stack, on a canvas
 *  attached to document.body so WebView2 resolves the full system font chain
 *  (a detached canvas / OffscreenCanvas silently misses system emoji — see
 *  packages/ridge-term/src/render/glyph_rasterizer.rs). The probe MUST measure
 *  the exact production stack ({@link SYSTEM_EMOJI_FALLBACK}) — no 'Flag Emoji'
 *  (not yet injected) and no 'Noto Color Emoji': naming a flag-capable font the
 *  production stack omits would let a machine with that font installed test
 *  "supported" and skip the subset, leaving flags as letter-boxes. Measure
 *  exactly what we ship, so the verdict matches what the terminal will render. */
function measureWithCanvas(text: string): number {
  const canvas = document.createElement('canvas');
  canvas.setAttribute(
    'style',
    'position:absolute;left:-9999px;top:-9999px;width:0;height:0;visibility:hidden;pointer-events:none',
  );
  document.body.appendChild(canvas);
  try {
    const ctx = canvas.getContext('2d');
    if (!ctx) return 0;
    ctx.font = `64px ${SYSTEM_EMOJI_FALLBACK},sans-serif`;
    return ctx.measureText(text).width;
  } finally {
    canvas.remove();
  }
}

/** Idempotently inject the flag-subset @font-face into <head>. */
function injectFlagFontFace(): void {
  if (document.getElementById('ridge-flag-emoji-face')) return;
  const style = document.createElement('style');
  style.id = 'ridge-flag-emoji-face';
  style.textContent = FLAG_FONT_FACE_CSS;
  document.head.appendChild(style);
}
