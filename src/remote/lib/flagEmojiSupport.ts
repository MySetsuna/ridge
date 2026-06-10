// Flag-emoji support detection + on-demand subset-face registration for the
// web-remote. The pure logic (probe + cache) is split from the browser glue so
// it stays unit-testable under the `node` vitest environment.

/** unicode-range-gated @font-face for the flag-only subset. Injected ONLY when
 *  the OS can't render flags; the browser then downloads /fonts/flags.woff2
 *  lazily — only when a flag codepoint actually appears. Note: the caller must
 *  also prepend 'Flag Emoji' to the target element's font-family stack — the
 *  @font-face declaration alone does not place the family into the cascade. */
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
