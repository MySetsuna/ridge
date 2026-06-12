// Single source of truth for the terminal font stack + emoji-fallback ordering.
//
// Shared by every terminal renderer entry point so the emoji policy lives in ONE
// place instead of being copy-pasted (and drifting) across:
//   - the desktop renderer default  (manager.ts)
//   - the desktop theme/font bridge  (themeBridge.ts `pushFont`)
//   - the web-remote controller      (src/remote/lib/terminalController.ts)
//
// Policy: use system color-emoji fonts only (Apple Color Emoji / Segoe UI
// Emoji). Country flags — which Windows' Segoe UI Emoji can't draw, and which
// the browser refuses to fall back for (Segoe carries Regional-Indicator LETTER
// glyphs, so it claims to render 🇯 as a boxed "J") — are handled by an
// on-demand, unicode-range-gated 'Flag Emoji' subset face. That face is injected
// ONLY when a capability probe finds the OS lacks native flags (see
// ./flagEmojiSupport `ensureFlagFont`); when injected, 'Flag Emoji' is placed
// FIRST in the chain so flag codepoints hit it before Segoe's letter glyphs.
// Desktop and web-remote share the exact same mechanism + this exact same stack.
// Text codepoints resolve to the mono/CJK fonts first.

/** Color-emoji fonts (system fonts only). */
export const EMOJI_FALLBACK = "'Apple Color Emoji','Segoe UI Emoji'";

/** Back-compat alias — identical to {@link EMOJI_FALLBACK} now that no bundled
 *  Noto exists. Kept so existing remote imports don't churn. */
export const SYSTEM_EMOJI_FALLBACK = EMOJI_FALLBACK;

/** Monospace + CJK text fonts (no emoji), in priority order. */
export const TEXT_MONO =
	"'JetBrains Mono','Cascadia Code','SF Mono',ui-monospace,Consolas,'SimHei','Heiti SC','Microsoft YaHei'";

/** Full default terminal font stack: text fonts → system emoji chain → generic.
 *  Used as the pre-bridge default (no flag face yet — the probe runs at bridge
 *  setup and `withEmojiFallback` re-derives the stack with 'Flag Emoji' first
 *  when the OS lacks native flags). */
export const DEFAULT_TERM_FONT = `${TEXT_MONO},${EMOJI_FALLBACK},monospace`;

/** Back-compat alias of {@link DEFAULT_TERM_FONT} (desktop + remote now share
 *  one stack). Kept so remote imports/tests don't churn. */
export const REMOTE_TERM_FONT = DEFAULT_TERM_FONT;

/** Family name of the on-demand flag-only subset face (see ./flagEmojiSupport). */
export const FLAG_EMOJI_FAMILY = "'Flag Emoji'";

const EMOJI_FAMILY_NAMES = new Set([
	'noto color emoji',  // legacy — stripped if present in user settings
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
 * Normalize any terminal font-family string so it ends with the canonical emoji
 * chain + a generic fallback. Strips any emoji/flag families the input already
 * names (regardless of order) and any trailing generic, then appends the chain:
 * system emoji by default, with `'Flag Emoji'` placed FIRST when
 * `flagFaceInjected` (true ⇔ the OS lacks native flags and the subset face was
 * injected) so flag codepoints hit it before Segoe's non-rendering
 * Regional-Indicator letter glyphs — the `unicode-range` on the @font-face keeps
 * it from affecting any other emoji. So a user-chosen mono font (which may carry
 * no emoji fonts at all) still gets color emoji + flags. Empty (or
 * emoji-only) input → the full {@link DEFAULT_TERM_FONT} (plus flags when
 * available).
 *
 * Desktop (themeBridge `pushFont`) and web-remote (terminalController) call this
 * SAME function with their probe's verdict, so both surfaces render identically.
 */
export function withEmojiFallback(family: string, flagFaceInjected = false): string {
	const tail = flagFaceInjected
		? `${FLAG_EMOJI_FAMILY},${EMOJI_FALLBACK},monospace`
		: `${EMOJI_FALLBACK},monospace`;
	const trimmed = (family ?? '').trim();
	if (trimmed === '') return `${TEXT_MONO},${tail}`;
	const kept = stripEmojiAndGeneric(trimmed);
	if (kept.length === 0) return `${TEXT_MONO},${tail}`;
	return `${kept.join(',')},${tail}`;
}

/**
 * @deprecated Use {@link withEmojiFallback} — desktop and remote now share one
 * function. Thin alias kept only to avoid breaking older imports.
 */
export const withRemoteEmojiFallback = withEmojiFallback;
