// Single source of truth for the terminal font stack + emoji-fallback ordering.
//
// Shared by every terminal renderer entry point so the emoji policy lives in ONE
// place instead of being copy-pasted (and drifting) across:
//   - the desktop renderer default  (manager.ts)
//   - the desktop theme/font bridge  (themeBridge.ts `pushFont`)
//   - the web-remote controller      (src/remote/lib/terminalController.ts)
//
// Policy: the BUNDLED "Noto Color Emoji" (COLRv1, see app.html / the remote
// index `@font-face` + /fonts/NotoColorEmoji.ttf) is placed AHEAD of the OS
// emoji fonts so every emoji — crucially incl. country flags, which Windows'
// Segoe UI Emoji has no glyphs for — renders from the same complete, Warp-level
// color font on every platform. Text codepoints resolve to the mono/CJK fonts
// first (Noto carries no Latin/CJK), so only true emoji reach Noto.

/** Color-emoji fonts, bundled Noto first then OS fonts as fallbacks. */
export const EMOJI_FALLBACK = "'Noto Color Emoji','Apple Color Emoji','Segoe UI Emoji'";

/** Monospace + CJK text fonts (no emoji), in priority order. */
export const TEXT_MONO =
	"'JetBrains Mono','Cascadia Code','SF Mono',ui-monospace,Consolas,'SimHei','Heiti SC','Microsoft YaHei'";

/** Full default terminal font stack: text fonts → emoji chain → generic. */
export const DEFAULT_TERM_FONT = `${TEXT_MONO},${EMOJI_FALLBACK},monospace`;

const EMOJI_FAMILY_NAMES = new Set([
	'noto color emoji',
	'apple color emoji',
	'segoe ui emoji',
	'flag emoji',
]);

/** Strip all emoji families and any trailing 'monospace' generic from a comma-separated font-family string. */
function stripEmojiAndGeneric(family: string): string[] {
	return family
		.split(',')
		.map((s) => s.trim())
		.filter(Boolean)
		.filter((p) => {
			const bare = p.replace(/^["']|["']$/g, '').toLowerCase();
			return !EMOJI_FAMILY_NAMES.has(bare) && bare !== 'monospace';
		});
}

/**
 * Normalize any terminal font-family string so it ends with the canonical
 * Noto-first emoji chain + a generic fallback. Strips any emoji families the
 * input already names (regardless of their order — e.g. a stale Noto-last
 * stack) and any trailing generic, then appends the chain. So a user-chosen
 * mono font (which may carry no emoji fonts at all) still gets color emoji,
 * and an old Noto-last stack is corrected to Noto-first. Empty input → the
 * full {@link DEFAULT_TERM_FONT}.
 */
export function withEmojiFallback(family: string): string {
	const trimmed = (family ?? '').trim();
	if (trimmed === '') return DEFAULT_TERM_FONT;
	const kept = stripEmojiAndGeneric(trimmed);
	return `${kept.join(',')},${EMOJI_FALLBACK},monospace`;
}

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
 * `flagFaceInjected` (true ⇔ the OS lacks native flags and the subset face
 * was injected) so flag codepoints hit it before Segoe's non-rendering
 * Regional-Indicator letter glyphs (the unicode-range on the @font-face keeps
 * it from affecting any other emoji). Empty input → the full
 * {@link REMOTE_TERM_FONT} (plus flags when available).
 */
export function withRemoteEmojiFallback(family: string, flagFaceInjected: boolean): string {
	const tail = flagFaceInjected
		? `${FLAG_EMOJI_FAMILY},${SYSTEM_EMOJI_FALLBACK},monospace`
		: `${SYSTEM_EMOJI_FALLBACK},monospace`;
	const trimmed = (family ?? '').trim();
	if (trimmed === '') return `${TEXT_MONO},${tail}`;
	const kept = stripEmojiAndGeneric(trimmed);
	if (kept.length === 0) return `${TEXT_MONO},${tail}`;
	return `${kept.join(',')},${tail}`;
}
