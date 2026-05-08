// Twemoji color emoji webfont registration.
//
// Why this exists: Tauri's WebView2 (Chromium) on Windows defaults to
// Segoe UI Emoji for color emoji rendering. On Windows 10 and many
// Windows 11 setups Segoe UI Emoji renders the older 2D / cartoon
// style that users perceive as dated next to modern terminals
// (iTerm2, Alacritty + a webfont, Discord, etc. all ship Twemoji or
// equivalent). Bundling Twemoji as a COLR/CPAL webfont lets the
// `OffscreenCanvas`-based WebGPU rasterizer AND the Canvas2D direct
// `fillText` path BOTH pick it up via `document.fonts` once the
// FontFace is added — main-thread `OffscreenCanvas` reads the
// document's font face set since Chrome 70+.
//
// `unicode-range` is critical: it restricts the browser to consulting
// Twemoji ONLY for emoji codepoints. Latin / CJK / box-drawing stay
// on the primary monospace (JetBrains Mono / Cascadia Code), so the
// terminal's text rendering is untouched. The ranges mirror
// `wcwidth.rs::is_color_emoji_codepoint` — the Rust-side heuristic
// the Canvas2D backend uses to decide whether to stretch a wide cell
// — so what the browser draws and what the renderer sizes for stay
// in sync.
//
// Idempotency: `registerTwemoji` is safe to call repeatedly. The
// first call kicks off the network/disk load and registers the
// FontFace; subsequent calls early-return.

import twemojiUrl from 'twemoji-colr-font/twemoji.woff2?url';

const TWEMOJI_FAMILY = 'Twemoji';

const TWEMOJI_UNICODE_RANGE =
	'U+1F004, U+1F0CF, U+1F1E6-1F1FF, U+1F200-1F251, U+1F300-1FBFF, U+2600-27BF, U+200D, U+FE00-FE0F';

let registered = false;

export async function registerTwemoji(): Promise<void> {
	if (registered) return;
	registered = true;

	if (typeof document === 'undefined' || !('fonts' in document)) return;

	const face = new FontFace(TWEMOJI_FAMILY, `url(${twemojiUrl}) format('woff2')`, {
		display: 'swap',
		unicodeRange: TWEMOJI_UNICODE_RANGE,
	});

	try {
		// Order matters: add() FIRST while status is 'unloaded', then
		// load(). This makes the FontFaceSet enter 'loading' state and
		// fire `loadingdone` when the load resolves — which the
		// TerminalManager constructor listens for to invalidate any
		// pane atlases caching glyphs against system Segoe UI Emoji.
		// If we did load() first, the FontFace would be 'loaded'
		// before add(), the set would never transition, and existing
		// rasterized emoji bitmaps would stay frozen at Segoe forever.
		document.fonts.add(face);
		await face.load();

		// Belt-and-suspenders: WebView2 has been observed to debounce
		// or skip `loadingdone` in some configurations. Directly poke
		// the manager singleton if it already exists so any panes
		// streaming output during the load gap get re-rasterized
		// against Twemoji on the next frame. If the singleton hasn't
		// been created yet (no pane attached), this is a no-op — a
		// future TerminalManager construction will start with a fresh
		// atlas, picking up Twemoji on its first miss.
		const { TerminalManager } = await import('$lib/terminal/manager');
		TerminalManager.tryInstance()?.invalidateAllPanes();

		// One-time confirmation so users can verify in devtools that
		// the webfont actually loaded. Filter on `[twemoji]` in the
		// console.
		// eslint-disable-next-line no-console
		console.info('[twemoji] color emoji webfont loaded');
	} catch (err) {
		// Loading failure is non-fatal — the font-family chain still
		// falls back to "Segoe UI Emoji" / "Apple Color Emoji" / "Noto
		// Color Emoji" for any emoji codepoint. We log so a regression
		// (asset path broken, network error in a corp env) doesn't
		// silently revert to the dated style.
		// eslint-disable-next-line no-console
		console.warn('[twemoji] failed to load color emoji webfont:', err);
	}
}
