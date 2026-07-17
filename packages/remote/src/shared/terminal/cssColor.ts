// src/lib/utils/cssColor.ts
//
// Normalize any browser-accepted CSS color string (hex, rgb, rgba, named,
// oklch, …) into the `#RRGGBB` / `#RRGGBBAA` form Monaco's theme API and
// the wasm terminal kernel's `parse_hex_color` both accept.
//
// Two callers share this:
//   - src/lib/terminal/themeBridge.ts — pushes Ridge's CSS vars into the
//     wasm kernel's xterm.js-shape Theme.
//   - src/lib/monaco/ridgeTheme.ts    — defines per-Ridge-theme Monaco
//     color overrides that match `--rg-bg` exactly.
//
// Single canvas-2d normalization context — re-used across every call so
// we don't allocate a fresh canvas per color lookup. Returns null when
// document is unavailable (SSR / pre-mount) or the input is unparseable.

let _normCtx: CanvasRenderingContext2D | null = null;

function getCtx(): CanvasRenderingContext2D | null {
	if (typeof document === 'undefined') return null;
	if (_normCtx) return _normCtx;
	const c = document.createElement('canvas');
	_normCtx = c.getContext('2d');
	return _normCtx;
}

/**
 * Parse a CSS color string into its 8-bit RGBA components, or null on
 * failure. Use this when you need to manipulate channels (e.g. apply a
 * different alpha) before re-formatting.
 */
function parseToRgba(css: string): { r: number; g: number; b: number; a: number } | null {
	if (!css) return null;
	const trimmed = css.trim();
	if (!trimmed) return null;

	const ctx = getCtx();
	if (!ctx) return null;

	// Reset to known-good first so unparseable input doesn't silently
	// inherit the prior successful parse.
	ctx.fillStyle = '#000000';
	ctx.fillStyle = trimmed;
	const out = ctx.fillStyle as string;

	if (out.startsWith('#')) {
		// Browser returns #RRGGBB for opaque colors.
		if (out.length !== 7) return null;
		const r = parseInt(out.slice(1, 3), 16);
		const g = parseInt(out.slice(3, 5), 16);
		const b = parseInt(out.slice(5, 7), 16);
		return { r, g, b, a: 255 };
	}
	const m = out.match(/^rgba?\((\d+),\s*(\d+),\s*(\d+)(?:,\s*([\d.]+))?\)$/);
	if (!m) return null;
	const r = parseInt(m[1], 10);
	const g = parseInt(m[2], 10);
	const b = parseInt(m[3], 10);
	const a = m[4] !== undefined ? Math.round(parseFloat(m[4]) * 255) : 255;
	return { r, g, b, a };
}

const toHex = (n: number): string =>
	Math.max(0, Math.min(255, n)).toString(16).padStart(2, '0');

/**
 * Normalize a CSS color string to `#RRGGBBAA`. Returns null when the
 * input cannot be parsed (SSR, malformed input). Opaque colors get
 * alpha `ff` appended.
 */
export function hex8(input: string): string | null {
	const rgba = parseToRgba(input);
	if (!rgba) return null;
	return `#${toHex(rgba.r)}${toHex(rgba.g)}${toHex(rgba.b)}${toHex(rgba.a)}`;
}

/**
 * Normalize a CSS color string to `#RRGGBBAA`, replacing the parsed
 * alpha with the given `alpha` (0..1). Returns null on parse failure.
 *
 * Use this when you have a base CSS variable (typically a hex from
 * `--rg-accent`) and need to render it semi-transparently — Monaco's
 * `editor.selectionBackground` etc. expect `#RRGGBBAA`.
 */
export function hex8WithAlpha(input: string, alpha: number): string | null {
	const rgba = parseToRgba(input);
	if (!rgba) return null;
	const a = Math.round(Math.max(0, Math.min(1, alpha)) * 255);
	return `#${toHex(rgba.r)}${toHex(rgba.g)}${toHex(rgba.b)}${toHex(a)}`;
}
