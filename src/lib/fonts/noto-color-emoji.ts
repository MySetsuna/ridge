// Noto Color Emoji (Google) — color emoji webfont registration.
//
// Why this exists: WebView2 (Chromium) on Windows defaults to Segoe UI
// Emoji for color emoji. Many users find Segoe's older 2D / cartoon
// style dated next to modern terminals (iTerm2, Alacritty + a webfont,
// VS Code, Discord) which all ship a custom color-emoji webfont.
// Noto Color Emoji is Google's COLRv1 vector emoji font (v39+, Feb
// 2026 build via @fontsource); Chromium 100+ renders COLRv1 directly,
// so the OffscreenCanvas-based WebGPU rasterizer AND the Canvas2D
// `fillText` path BOTH pick it up via `document.fonts` once the
// FontFace set finishes loading. Main-thread `OffscreenCanvas` reads
// the document's font face set since Chrome 70+.
//
// We import the fontsource @font-face CSS directly so Vite emits all
// 11 woff2 subsets to dist and stitches up correct asset URLs. Each
// @font-face has a tight `unicode-range` so the browser only fetches
// the subset(s) actually used by emoji codepoints in flight — typical
// terminal usage (a few common emoji) downloads 1-2 subsets, not all
// 11. Total payload caps around 25 MB worst case (every subset
// loaded), versus 470 KB for the prior single-file Twemoji bundle —
// trade-off acceptable for a desktop Tauri app where the visual
// upgrade is the user-visible win.
//
// Idempotency: `registerNotoColorEmoji` is safe to call repeatedly.
// First call awaits `document.fonts.ready`; later calls early-return.

import '@fontsource/noto-color-emoji/index.css';

let registered = false;

export async function registerNotoColorEmoji(): Promise<void> {
	if (registered) return;
	registered = true;

	if (typeof document === 'undefined' || !('fonts' in document)) return;

	try {
		// `document.fonts.ready` resolves once every @font-face declared
		// at this point has either loaded its first subset or failed.
		// Browser will only have actually FETCHED the subsets whose
		// unicode-range matched a glyph the page tried to render — so
		// for an idle session this resolves without network IO.
		await document.fonts.ready;

		// Belt-and-suspenders: WebView2 has been observed to debounce
		// or skip `loadingdone` in some configurations. Directly poke
		// the manager singleton if it already exists so any panes
		// streaming output during the load gap get re-rasterized
		// against Noto on the next frame. If the singleton hasn't
		// been created yet (no pane attached), this is a no-op — a
		// future TerminalManager construction will start with a fresh
		// atlas, picking up Noto on its first miss.
		const { TerminalManager } = await import('$lib/terminal/manager');
		TerminalManager.tryInstance()?.invalidateAllPanes();

		// One-time confirmation so users can verify in devtools that
		// the webfont actually loaded. Filter on `[noto-emoji]` in
		// the console.
		// eslint-disable-next-line no-console
		console.info('[noto-emoji] color emoji webfont ready');
	} catch (err) {
		// Loading failure is non-fatal — the font-family chain still
		// falls back to "Segoe UI Emoji" / "Apple Color Emoji" / system
		// "Noto Color Emoji" for any emoji codepoint. We log so a
		// regression (asset path broken, network error in a corp env)
		// doesn't silently revert to the dated style.
		// eslint-disable-next-line no-console
		console.warn('[noto-emoji] failed to load color emoji webfont:', err);
	}
}
