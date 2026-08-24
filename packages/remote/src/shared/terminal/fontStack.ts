// Single source of truth for the terminal font stack + fallback ordering.
//
// Shared by every terminal renderer entry point so the emoji policy lives in ONE
// place instead of being copy-pasted (and drifting) across:
//   - the desktop renderer default  (manager.ts)
//   - the desktop theme/font bridge  (themeBridge.ts `pushFont`)
//   - the web-remote controller      (src/remote/lib/terminalController.ts)
//
// Policy: selected installed mono/CJK faces first, then generic text, symbols,
// and emoji system faces. These are names only; loaders still install local
// font files before Wasm starts.
// Raw faces are supplied to the Wasm rasterizer by the native or browser-local
// font-data service; document webfonts are deliberately outside this path.

/** General Unicode text fallback faces (system fonts only). */
export const TEXT_FALLBACK = "'Segoe UI','Noto Sans','Arial Unicode MS'";

/** Unicode symbol fonts (system fonts only). */
export const SYMBOL_FALLBACK =
	"'Segoe UI Symbol','Segoe UI Historic','Apple Symbols','Noto Sans Symbols 2','Noto Sans Symbols2','Noto Sans Math'";

/** Color-emoji fonts (system fonts only). */
export const EMOJI_FALLBACK = "'Noto Color Emoji','Apple Color Emoji','Segoe UI Emoji'";

/** Back-compat alias — identical to {@link EMOJI_FALLBACK} now that no bundled
 *  Noto exists. Kept so existing remote imports don't churn. */
export const SYSTEM_EMOJI_FALLBACK = EMOJI_FALLBACK;

/** Monospace + CJK text fonts (no emoji), in priority order. */
export const TEXT_MONO =
	"'JetBrains Mono','Cascadia Code','SF Mono',ui-monospace,Consolas,'SimHei','Heiti SC','Microsoft YaHei'";

/** Full default terminal font stack: text → symbols → emoji → generic. */
export const DEFAULT_TERM_FONT = `${TEXT_MONO},${TEXT_FALLBACK},${SYMBOL_FALLBACK},${EMOJI_FALLBACK},monospace`;

/** Back-compat alias of {@link DEFAULT_TERM_FONT} (desktop + remote now share
 *  one stack). Kept so remote imports/tests don't churn. */
export const REMOTE_TERM_FONT = DEFAULT_TERM_FONT;

const EMOJI_FAMILY_NAMES = new Set([
	'noto color emoji',  // legacy — stripped if present in user settings
	'apple color emoji',
	'segoe ui emoji',
	'flag emoji',
]);

const SYMBOL_FAMILY_NAMES = new Set([
	'segoe ui symbol',
	'segoe ui historic',
	'apple symbols',
	'noto sans symbols 2',
	'noto sans symbols2',
	'noto sans math',
]);

const TEXT_FAMILY_NAMES = new Set([
	'segoe ui',
	'noto sans',
	'arial unicode ms',
]);

/** Strip canonical symbol/emoji families and generic fallbacks from a stack. */
function stripCanonicalFallbacks(family: string): string[] {
	return family
		.split(',')
		.map((s) => s.trim())
		.filter(Boolean)
		.filter((p) => {
			const bare = p.replace(/^["']|["']$/g, '').toLowerCase();
			return !EMOJI_FAMILY_NAMES.has(bare)
				&& !SYMBOL_FAMILY_NAMES.has(bare)
				&& !TEXT_FAMILY_NAMES.has(bare)
				&& bare !== 'monospace';
		});
}

/**
 * Normalize any terminal font-family string so it ends with the canonical emoji
 * chain + a generic fallback. Legacy webfont family names are stripped because
 * the Wasm renderer accepts raw installed faces, not document font faces.
 *
 * Desktop (themeBridge `pushFont`) and web-remote (terminalController) call this
 * SAME function, so both surfaces resolve the same installed family order.
 */
export function withEmojiFallback(family: string): string {
	const tail = `${TEXT_FALLBACK},${SYMBOL_FALLBACK},${EMOJI_FALLBACK},monospace`;
	const trimmed = (family ?? '').trim();
	if (trimmed === '') return `${TEXT_MONO},${tail}`;
	const kept = stripCanonicalFallbacks(trimmed);
	if (kept.length === 0) return `${TEXT_MONO},${tail}`;
	return `${kept.join(',')},${tail}`;
}

/**
 * @deprecated Use {@link withEmojiFallback} — desktop and remote now share one
 * function. Thin alias kept only to avoid breaking older imports.
 */
export const withRemoteEmojiFallback = withEmojiFallback;
