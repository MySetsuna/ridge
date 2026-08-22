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
// A hidden DOM element delegates CSS parsing to the browser without acquiring
// a Canvas2D context. Returns null during SSR/pre-mount or for invalid input.

let _normElement: HTMLSpanElement | null = null;

function getNormElement(): HTMLSpanElement | null {
	if (typeof document === 'undefined' || !document.documentElement) return null;
	if (_normElement?.isConnected) return _normElement;
	const element = document.createElement('span');
	element.setAttribute('aria-hidden', 'true');
	element.style.cssText = 'position:fixed;visibility:hidden;pointer-events:none;contain:strict';
	document.documentElement.appendChild(element);
	_normElement = element;
	return element;
}

function parseSerializedColor(value: string): { r: number; g: number; b: number; a: number } | null {
	const hex = /^#([0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})$/i.exec(value);
	if (hex) {
		let digits = hex[1];
		if (digits.length <= 4) digits = [...digits].map((digit) => digit + digit).join('');
		if (digits.length === 6) digits += 'ff';
		return {
			r: Number.parseInt(digits.slice(0, 2), 16),
			g: Number.parseInt(digits.slice(2, 4), 16),
			b: Number.parseInt(digits.slice(4, 6), 16),
			a: Number.parseInt(digits.slice(6, 8), 16),
		};
	}

	const rgb = /^rgba?\(\s*([\d.]+)\s*(?:,\s*|\s+)([\d.]+)\s*(?:,\s*|\s+)([\d.]+)(?:\s*(?:,|\/)\s*([\d.]+%?))?\s*\)$/i.exec(value);
	if (!rgb) return null;
	return {
		r: Math.round(Number.parseFloat(rgb[1])),
		g: Math.round(Number.parseFloat(rgb[2])),
		b: Math.round(Number.parseFloat(rgb[3])),
		a: rgb[4] === undefined
			? 255
			: Math.round(Number.parseFloat(rgb[4]) * (rgb[4].endsWith('%') ? 2.55 : 255)),
	};
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
	const direct = parseSerializedColor(trimmed);
	if (direct) return direct;

	const element = getNormElement();
	if (!element || typeof getComputedStyle !== 'function') return null;
	element.style.color = '';
	element.style.color = trimmed;
	if (!element.style.color) return null;
	const out = getComputedStyle(element).color.trim();
	return parseSerializedColor(out);
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
