# Remote Emoji Font Diet Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop the remote (web/mobile) client from fetching a 4.8MB color-emoji font on first paint; render ordinary emoji from the OS and lazily fetch a tiny flag-only subset only on systems that can't draw flags (Windows/WebView2).

**Architecture:** Two gates. (1) A one-shot JS capability probe decides whether the OS renders Regional-Indicator flags; only when it can't do we register a `@font-face`. (2) That `@font-face` carries `unicode-range`, so the browser downloads the subset only when a flag codepoint actually appears. The remote font stack drops the bundled Noto and puts the on-demand `'Flag Emoji'` family first (ahead of Segoe's non-rendering RI letter glyphs) gated by the range. Desktop is untouched.

**Tech Stack:** TypeScript, Svelte, Vitest (node env), Vite; `pyftsubset` (fonttools) for the subset build; the remote terminal rasterizes via an attached-`<canvas>` `fillText` in `packages/ridge-term` (so standard CSS `@font-face` + `unicode-range` apply).

**Spec:** `docs/superpowers/specs/2026-06-10-remote-emoji-font-diet-design.md`

---

## File Structure

- **Create** `scripts/build-flag-font.mjs` — reproducible subset build (pyftsubset → `flags.woff2`), enforces the 800KB red line.
- **Create** `src/remote/public/fonts/flags.woff2` — flag-only subset (build artifact).
- **Create** `src/remote/lib/flagEmojiSupport.ts` — capability probe + cache (pure, unit-tested) and the browser glue (`ensureRemoteFlagFont`).
- **Create** `src/remote/lib/flagEmojiSupport.test.ts` — unit tests for the pure logic.
- **Modify** `src/lib/terminal/fontStack.ts` — add the remote variant constants + `withRemoteEmojiFallback` (desktop exports unchanged).
- **Create** `src/lib/terminal/fontStack.test.ts` — unit tests for `withRemoteEmojiFallback`.
- **Modify** `src/remote/lib/terminalController.ts` — use the remote stack + run the probe at construction.
- **Modify** `src/remote/index.html` — delete the 4.8MB `@font-face` and the preload script.
- **Delete** `src/remote/public/fonts/NotoColorEmoji.ttf` — the 4.8MB full font (remote copy).

---

## Task 1: Flag-only subset font + build script (red-line gate)

This is the first step on purpose — if the subset can't get under 800KB, we re-evaluate before touching any code (design §7).

**Files:**
- Create: `scripts/build-flag-font.mjs`
- Create (artifact): `src/remote/public/fonts/flags.woff2`
- Source (must exist, kept for desktop): `static/fonts/NotoColorEmoji.ttf`

- [ ] **Step 1: Ensure fonttools + brotli are available**

Run: `pyftsubset --help >/dev/null && python -c "import brotli; print('brotli ok')"`
Expected: prints `brotli ok` (woff2 flavor needs brotli). If it errors: `pip install fonttools brotli`.

- [ ] **Step 2: Write the build script**

Create `scripts/build-flag-font.mjs`:

```js
// Build the flag-only color-emoji subset for the web-remote.
//
// Source: static/fonts/NotoColorEmoji.ttf (the desktop's bundled COLRv1 face,
// kept in-repo). Output: src/remote/public/fonts/flags.woff2 — only Regional
// Indicator pairs + subdivision-flag tag sequences, so it stays tiny and is
// fetched on-demand via a unicode-range @font-face.
//
// Requires fonttools on PATH:  pip install fonttools brotli
import { execFileSync } from 'node:child_process';
import { existsSync, statSync, mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const src = resolve(root, 'static/fonts/NotoColorEmoji.ttf');
const out = resolve(root, 'src/remote/public/fonts/flags.woff2');
const MAX_BYTES = 800 * 1024; // design §7 red line

if (!existsSync(src)) {
  console.error(`Source font missing: ${src}`);
  process.exit(1);
}
mkdirSync(dirname(out), { recursive: true });

// --layout-features=* keeps GSUB so Noto can ligate the RI pairs (and the
// U+1F3F4 + tag sequences) into single flag glyphs. COLR/CPAL ride along with
// the kept glyphs so the flags stay colored.
execFileSync(
  'pyftsubset',
  [
    src,
    '--unicodes=U+1F1E6-1F1FF,U+1F3F4,U+E0020-E007F',
    '--layout-features=*',
    '--flavor=woff2',
    `--output-file=${out}`,
  ],
  { stdio: 'inherit' },
);

const bytes = statSync(out).size;
const kb = (bytes / 1024).toFixed(1);
if (bytes > MAX_BYTES) {
  console.error(
    `flags.woff2 is ${kb} KB — exceeds the ${MAX_BYTES / 1024} KB red line. ` +
      `Re-evaluate (design §7).`,
  );
  process.exit(1);
}
console.log(`flags.woff2: ${kb} KB (<= ${MAX_BYTES / 1024} KB) OK`);
```

- [ ] **Step 3: Run the build and verify the red line**

Run: `node scripts/build-flag-font.mjs`
Expected: prints `flags.woff2: <N> KB (<= 800 KB) OK` and exits 0. If it exceeds 800KB, STOP and report — do not proceed (design §7 exit condition).

- [ ] **Step 4: Manually verify the subset renders colored flags (quick visual check)**

Run: `node -e "const f=require('fs');const b=f.statSync('src/remote/public/fonts/flags.woff2').size;console.log('size',b);"`
Then confirm the file is a valid woff2 (starts with `wOF2`):
Run: `node -e "const f=require('fs');const h=f.readFileSync('src/remote/public/fonts/flags.woff2').subarray(0,4).toString('latin1');console.log(h); process.exit(h==='wOF2'?0:1)"`
Expected: prints `wOF2` and exits 0.

- [ ] **Step 5: Register the build script in package.json**

Modify `package.json` scripts (add this line alongside the other `build:*` entries):

```json
"build:flag-font": "node scripts/build-flag-font.mjs",
```

- [ ] **Step 6: Commit**

```bash
git add scripts/build-flag-font.mjs src/remote/public/fonts/flags.woff2 package.json
git commit -m "build(remote): flag-only emoji subset + reproducible build script"
```

---

## Task 2: Remote font-stack variant in fontStack.ts (TDD)

Pure string logic — fully unit-testable under the node vitest env. Desktop exports (`DEFAULT_TERM_FONT`, `withEmojiFallback`, `EMOJI_FALLBACK`) are NOT modified.

**Files:**
- Test: `src/lib/terminal/fontStack.test.ts`
- Modify: `src/lib/terminal/fontStack.ts`

- [ ] **Step 1: Write the failing test**

Create `src/lib/terminal/fontStack.test.ts`:

```ts
import { describe, it, expect } from 'vitest';
import {
  withRemoteEmojiFallback,
  REMOTE_TERM_FONT,
  TEXT_MONO,
} from './fontStack';

describe('withRemoteEmojiFallback', () => {
  it('empty input, no flags → remote default (system emoji only)', () => {
    expect(withRemoteEmojiFallback('', false)).toBe(REMOTE_TERM_FONT);
  });

  it('empty input, flags available → Flag Emoji first, ahead of system emoji', () => {
    expect(withRemoteEmojiFallback('', true)).toBe(
      `${TEXT_MONO},'Flag Emoji','Apple Color Emoji','Segoe UI Emoji',monospace`,
    );
  });

  it('keeps a user mono font, strips stale emoji families, appends system chain', () => {
    expect(withRemoteEmojiFallback("'Fira Code','Noto Color Emoji'", false)).toBe(
      "'Fira Code','Apple Color Emoji','Segoe UI Emoji',monospace",
    );
  });

  it('strips an existing Flag Emoji family before re-appending (no dupes)', () => {
    expect(withRemoteEmojiFallback("'Fira Code','Flag Emoji'", true)).toBe(
      "'Fira Code','Flag Emoji','Apple Color Emoji','Segoe UI Emoji',monospace",
    );
  });

  it('REMOTE_TERM_FONT carries no bundled Noto', () => {
    expect(REMOTE_TERM_FONT).not.toContain('Noto');
  });
});
```

- [ ] **Step 2: Run it to verify it fails**

Run: `pnpm exec vitest run src/lib/terminal/fontStack.test.ts`
Expected: FAIL — `withRemoteEmojiFallback` / `REMOTE_TERM_FONT` are not exported.

- [ ] **Step 3: Implement the remote variant**

Modify `src/lib/terminal/fontStack.ts`. Add the `'flag emoji'` entry to the existing `EMOJI_FAMILY_NAMES` set:

```ts
const EMOJI_FAMILY_NAMES = new Set([
	'noto color emoji',
	'apple color emoji',
	'segoe ui emoji',
	'flag emoji',
]);
```

Then append these new exports at the end of the file (leave everything above untouched):

```ts
// ─────────────────────────── Remote (web/mobile) variant ───────────────────
//
// The remote does NOT bundle the full Noto webfont. Ordinary emoji render from
// the OS; country flags — which Windows' Segoe UI Emoji can't draw — come from
// an on-demand, unicode-range-gated 'Flag Emoji' subset face (registered by
// src/remote/lib/flagEmojiSupport.ts only when the OS lacks flags).

/** System color-emoji fonts only (no bundled Noto). */
export const SYSTEM_EMOJI_FALLBACK = "'Apple Color Emoji','Segoe UI Emoji'";

/** Family name of the on-demand flag-only subset face. */
export const FLAG_EMOJI_FAMILY = "'Flag Emoji'";

/** Remote default terminal font: text/CJK fonts → system emoji → generic. */
export const REMOTE_TERM_FONT = `${TEXT_MONO},${SYSTEM_EMOJI_FALLBACK},monospace`;

/**
 * Remote counterpart of {@link withEmojiFallback}. Strips any emoji/flag
 * families and trailing generic from `family`, then appends the remote emoji
 * chain: system emoji by default, with `'Flag Emoji'` placed FIRST when
 * `flagsAvailable` so flag codepoints hit it before Segoe's non-rendering
 * Regional-Indicator letter glyphs (the unicode-range on the @font-face keeps
 * it from affecting any other emoji). Empty input → the full
 * {@link REMOTE_TERM_FONT} (plus flags when available).
 */
export function withRemoteEmojiFallback(family: string, flagsAvailable: boolean): string {
	const tail = flagsAvailable
		? `${FLAG_EMOJI_FAMILY},${SYSTEM_EMOJI_FALLBACK},monospace`
		: `${SYSTEM_EMOJI_FALLBACK},monospace`;
	const trimmed = (family ?? '').trim();
	if (trimmed === '') return `${TEXT_MONO},${tail}`;
	const kept = trimmed
		.split(',')
		.map((s) => s.trim())
		.filter(Boolean)
		.filter((p) => {
			const bare = p.replace(/^["']|["']$/g, '').toLowerCase();
			return !EMOJI_FAMILY_NAMES.has(bare) && bare !== 'monospace';
		});
	return `${kept.join(',')},${tail}`;
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `pnpm exec vitest run src/lib/terminal/fontStack.test.ts`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add src/lib/terminal/fontStack.ts src/lib/terminal/fontStack.test.ts
git commit -m "feat(remote): remote font-stack variant (system emoji + on-demand flag face)"
```

---

## Task 3: Capability probe + cache, pure logic (TDD)

Split the testable logic (width-based probe, cache encode/decode with UA fingerprint) from any browser API so it runs under node vitest.

**Files:**
- Test: `src/remote/lib/flagEmojiSupport.test.ts`
- Create: `src/remote/lib/flagEmojiSupport.ts`

- [ ] **Step 1: Write the failing test**

Create `src/remote/lib/flagEmojiSupport.test.ts`:

```ts
import { describe, it, expect } from 'vitest';
import {
  probeSystemFlagSupport,
  readFlagCache,
  writeFlagCache,
} from './flagEmojiSupport';

describe('probeSystemFlagSupport', () => {
  // measure() is the only browser dependency; here it's mocked. A single
  // Regional Indicator '🇯' has String#length 2; the pair '🇯🇵' has length 4.
  it('merged flag glyph (pair ≈ single width) → supported', () => {
    const measure = (t: string) => (t.length > 2 ? 11 : 10);
    expect(probeSystemFlagSupport(measure)).toBe(true);
  });

  it('two letter glyphs (pair ≈ 2× single width) → not supported', () => {
    const measure = (t: string) => (t.length > 2 ? 20 : 10);
    expect(probeSystemFlagSupport(measure)).toBe(false);
  });

  it('unmeasurable (0 width) → assume supported (inject nothing)', () => {
    expect(probeSystemFlagSupport(() => 0)).toBe(true);
  });
});

describe('flag-support cache', () => {
  it('round-trips a verdict for the same UA fingerprint', () => {
    const raw = writeFlagCache(false, 'UA-1');
    expect(readFlagCache(raw, 'UA-1')).toBe(false);
  });

  it('invalidates when the UA fingerprint changes', () => {
    const raw = writeFlagCache(true, 'UA-1');
    expect(readFlagCache(raw, 'UA-2')).toBeNull();
  });

  it('returns null on empty / corrupt input', () => {
    expect(readFlagCache(null, 'UA')).toBeNull();
    expect(readFlagCache('{not json', 'UA')).toBeNull();
  });
});
```

- [ ] **Step 2: Run it to verify it fails**

Run: `pnpm exec vitest run src/remote/lib/flagEmojiSupport.test.ts`
Expected: FAIL — module `./flagEmojiSupport` does not exist.

- [ ] **Step 3: Implement the pure logic**

Create `src/remote/lib/flagEmojiSupport.ts`:

```ts
// Flag-emoji support detection + on-demand subset-face registration for the
// web-remote. The pure logic (probe + cache) is split from the browser glue so
// it stays unit-testable under the `node` vitest environment.

/** unicode-range-gated @font-face for the flag-only subset. Injected ONLY when
 *  the OS can't render flags; the browser then downloads /fonts/flags.woff2
 *  lazily — only when a flag codepoint actually appears. */
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
  if (!(single > 0) || !(pair > 0)) return true; // can't tell → assume supported
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
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `pnpm exec vitest run src/remote/lib/flagEmojiSupport.test.ts`
Expected: PASS (6 tests).

- [ ] **Step 5: Commit**

```bash
git add src/remote/lib/flagEmojiSupport.ts src/remote/lib/flagEmojiSupport.test.ts
git commit -m "feat(remote): flag-emoji capability probe + UA-fingerprinted cache (pure logic)"
```

---

## Task 4: Browser glue — probe/cache/inject orchestration

Browser-dependent (canvas, localStorage, DOM), so no node unit test; verified by `pnpm check` (types) here and manually in Task 5. Adds to the file from Task 3.

**Files:**
- Modify: `src/remote/lib/flagEmojiSupport.ts`

- [ ] **Step 1: Append the browser orchestration**

Add to the bottom of `src/remote/lib/flagEmojiSupport.ts`:

```ts
// ───────────────────────────── Browser glue ────────────────────────────────

/**
 * Resolve whether the remote needs the flag subset face, using a cached
 * verdict when present else a one-shot canvas probe, and register the
 * @font-face when the OS lacks flags. Returns `flagsAvailable` — whether the
 * 'Flag Emoji' family is now part of the document (so the font stack should
 * include it). Safe to call repeatedly (cache + idempotent injection). Never
 * throws; returns false in non-DOM contexts.
 */
export function ensureRemoteFlagFont(): boolean {
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
  if (!supported) injectFlagFontFace();
  return !supported;
}

/** Measure advance width of `text` under the system emoji stack, on a canvas
 *  attached to document.body so WebView2 resolves the full system font chain
 *  (a detached canvas / OffscreenCanvas silently misses system emoji — see
 *  packages/ridge-term/src/render/glyph_rasterizer.rs). Must NOT name
 *  'Flag Emoji' (not yet injected) so the probe reflects the OS, not us. */
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
    ctx.font = "64px 'Apple Color Emoji','Segoe UI Emoji',sans-serif";
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
```

- [ ] **Step 2: Type-check**

Run: `pnpm check`
Expected: no new errors referencing `flagEmojiSupport.ts`. (Pre-existing unrelated warnings are fine.)

- [ ] **Step 3: Re-run the unit tests (ensure pure logic still green)**

Run: `pnpm exec vitest run src/remote/lib/flagEmojiSupport.test.ts`
Expected: PASS (6 tests) — the added glue doesn't change the tested exports.

- [ ] **Step 4: Commit**

```bash
git add src/remote/lib/flagEmojiSupport.ts
git commit -m "feat(remote): ensureRemoteFlagFont — cached probe + on-demand @font-face injection"
```

---

## Task 5: Wire into the controller, strip the 4.8MB font, manual verify

**Files:**
- Modify: `src/remote/lib/terminalController.ts:3` (imports), `:15` (FONT_STACK), `:98` (font resolution)
- Modify: `src/remote/index.html` (remove the full `@font-face` + preload script)
- Delete: `src/remote/public/fonts/NotoColorEmoji.ttf`

- [ ] **Step 1: Swap the controller to the remote stack + run the probe**

In `src/remote/lib/terminalController.ts`, replace the import on line 3:

```ts
import { REMOTE_TERM_FONT, withRemoteEmojiFallback } from '$lib/terminal/fontStack';
import { ensureRemoteFlagFont } from './flagEmojiSupport';
```

Replace the `FONT_STACK` export (line 15 area) — keep the surrounding doc comment intent but point at the remote stack:

```ts
// Re-exported remote font stack (system emoji baseline; flags come from the
// on-demand 'Flag Emoji' subset face — see ./flagEmojiSupport). No bundled
// Noto webfont, so ordinary emoji cost zero extra bytes on the remote.
export const FONT_STACK = REMOTE_TERM_FONT;
```

In the constructor, replace line 98:

```ts
    // Probe once (cached): on a flag-less OS this registers the unicode-range
    // 'Flag Emoji' @font-face; the browser still only fetches flags.woff2 when
    // a flag codepoint actually appears. Pick the matching stack variant.
    const flagsAvailable = ensureRemoteFlagFont();
    this.fontFamily = withRemoteEmojiFallback(opts.fontFamily ?? '', flagsAvailable);
```

- [ ] **Step 2: Strip the full font from index.html**

In `src/remote/index.html`, delete the entire `@font-face` block inside `<style>` (the `/* Bundled color-emoji font … */` comment through the closing `}` of `@font-face{…font-display:swap;}`), leaving the `:root{…}` palette and the rest of the styles intact.

Then delete the preload `<script>` block (the one containing `document.fonts.load('64px "Noto Color Emoji"', '🇯🇵😀👍')`) entirely. Keep `<script type="module" src="./main.ts"></script>`.

- [ ] **Step 3: Delete the 4.8MB remote font**

```bash
git rm src/remote/public/fonts/NotoColorEmoji.ttf
```

- [ ] **Step 4: Type-check + full unit suite**

Run: `pnpm check`
Expected: no new errors in `terminalController.ts`.
Run: `pnpm test`
Expected: the new `fontStack` + `flagEmojiSupport` suites pass; no pre-existing suites break.

- [ ] **Step 5: Manual verification — Windows / WebView2 (flag-less OS)**

Run the remote dev server: `pnpm dev:remote`, open it in the desktop WebView2 / a Chromium browser on Windows, attach to a terminal pane, and with DevTools Network tab open:
- [ ] On first paint, **no** font request fires (no `flags.woff2`, no `NotoColorEmoji`).
- [ ] Ordinary emoji (😀👍🔥) render via the system (Segoe).
- [ ] Echo a flag — e.g. run `printf '\U0001F1EF\U0001F1F5 \U0001F3F4\U000E0067\U000E0062\U000E0073\U000E0063\U000E0074\U000E007F\n'` (🇯🇵 + Scotland) — and confirm: a single `flags.woff2` request appears **at that moment**, then both render as **colored flags** (font-display:swap may show a brief letter fallback first).
- [ ] Reload: verdict is cached (`localStorage['ridge.flagEmojiSupport']` present); behavior unchanged.

- [ ] **Step 6: Manual verification — macOS/iOS (flag-capable OS)**

Open the remote on macOS Safari/Chrome or iOS:
- [ ] Flags render (from the OS) and **no** `flags.woff2` request ever fires, even after echoing a flag.
- [ ] `localStorage['ridge.flagEmojiSupport']` shows `supported:true`.

- [ ] **Step 7: Regression — desktop untouched**

Run: `pnpm exec vitest run src/lib/terminal/fontStack.test.ts`
Expected: PASS. Confirm `git diff` shows no changes to `manager.ts` / `themeBridge.ts` and the desktop `DEFAULT_TERM_FONT` / `withEmojiFallback` are unchanged.

- [ ] **Step 8: Commit**

```bash
git add src/remote/lib/terminalController.ts src/remote/index.html
git commit -m "feat(remote): system-first emoji + on-demand flags; drop 4.8MB Noto webfont"
```

---

## Self-Review

**Spec coverage:**
- §3 double-gate (probe + unicode-range) → Task 3 (probe), Task 4 (inject + cache gate), Task 1 (`unicode-range` in the face CSS / subset). ✓
- §4 stack & no-fallback-trap (Flag Emoji first, range-limited) → Task 2 `withRemoteEmojiFallback` + Task 3 `FLAG_FONT_FACE_CSS`. ✓
- §5.1 subset font + §5.2 build script → Task 1. ✓
- §5.3 probe / §5.4 cache → Task 3 + Task 4. ✓
- §5.5 cleanup (index.html + delete TTF + controller) → Task 5. ✓
- §6 data flow → Task 5 Step 1 (probe→stack) + lazy fetch verified in Steps 5–6. ✓
- §7 800KB red line → Task 1 Step 3 (script enforces + exits non-zero). ✓
- §8 tests (subset size, Win/mac manual, probe unit) → Task 1 Step 3–4, Task 3 unit, Task 5 Steps 5–7. ✓
- §A subdivision flags included → Task 1 unicode list `U+1F3F4,U+E0020-E007F` + Task 5 Step 5 Scotland check. ✓

**Placeholder scan:** none — every code/step is concrete.

**Type consistency:** `withRemoteEmojiFallback(family, flagsAvailable)` (Task 2) ↔ called in Task 5 Step 1. `ensureRemoteFlagFont(): boolean` (Task 4) ↔ called in Task 5. `probeSystemFlagSupport/readFlagCache/writeFlagCache/FLAG_FONT_FACE_CSS/FLAG_CACHE_KEY` (Task 3) ↔ used in Task 4. `REMOTE_TERM_FONT` (Task 2) ↔ used in Task 5. Consistent. ✓
