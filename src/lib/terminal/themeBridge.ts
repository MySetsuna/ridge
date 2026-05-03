// src/lib/terminal/themeBridge.ts
//
// Push Ridge's CSS-variable theme into the wasm terminal kernel.
//
// Why this exists:
//   - `manager.ts` is deliberately host-agnostic. It carries an
//     `opts.theme: Record<string, string>` blob but doesn't know how
//     to derive one from Ridge's own CSS / Svelte stores.
//   - The wasm kernel's `Theme::apply_partial` accepts xterm.js-shape
//     keys (background, foreground, cursor, cursorAccent,
//     selectionBackground, hyperlinkColor, ANSI 16, …) but `parse_hex_color`
//     only understands `#RGB` / `#RRGGBB` / `#RRGGBBAA`. CSS variables can
//     hold any browser-accepted color form (rgba(), oklch(), named, ...),
//     so we normalize to hex8 here.
//   - Without this bridge, `attach()`'s `if (this.opts.theme)` guard
//     never fires, the wasm Theme stays at its compile-time default
//     `[0x55,0xaa,0xff,0x60]` selection blue (very off-brand for
//     Ridge's green accent palette) — TASKS Bug A.
//
// Lifecycle:
//   - `setupTerminalThemeBridge()` once at app boot (from +page.svelte
//     onMount). Subscribes to `settingsStore` so theme changes propagate
//     to all currently-attached panes via `manager.setTheme`. Returns an
//     unsubscribe; call from onDestroy if you ever tear down the bridge
//     (current Ridge keeps it alive for the whole app session).
//   - Re-callable: subsequent calls are no-ops if a subscription is
//     already active.

import { settingsStore } from '$lib/stores/settings';
import { TerminalManager } from './manager';

// Lazily-created canvas 2d context used to normalize ANY CSS color string
// into one of two canonical forms the browser returns:
//   - "#rrggbb" for opaque colors
//   - "rgba(r, g, b, a)" for translucent (alpha < 1)
// We then convert both to "#rrggbbaa" for the wasm parse_hex_color path.
let _normCtx: CanvasRenderingContext2D | null = null;

function normalizeColor(css: string): string | null {
	if (!css) return null;
	const trimmed = css.trim();
	if (!trimmed) return null;

	if (typeof document === 'undefined') return null;
	if (_normCtx === null) {
		const c = document.createElement('canvas');
		_normCtx = c.getContext('2d');
		if (!_normCtx) return null;
	}

	// Reset to a known-good value first so an unparseable input keeps
	// the reset value rather than the previous successful parse —
	// without this, invalid CSS would silently inherit a stale color.
	_normCtx.fillStyle = '#000000';
	_normCtx.fillStyle = trimmed;
	const out = _normCtx.fillStyle as string;

	if (out.startsWith('#')) {
		// Browser returns #RRGGBB for opaque colors. Append full alpha.
		return out.length === 7 ? `${out}ff` : out;
	}
	// Translucent — "rgba(r, g, b, a)" form.
	const m = out.match(/^rgba?\((\d+),\s*(\d+),\s*(\d+)(?:,\s*([\d.]+))?\)$/);
	if (!m) return null;
	const r = parseInt(m[1], 10);
	const g = parseInt(m[2], 10);
	const b = parseInt(m[3], 10);
	const a = m[4] !== undefined ? Math.round(parseFloat(m[4]) * 255) : 255;
	const hex = (n: number) => n.toString(16).padStart(2, '0');
	return `#${hex(r)}${hex(g)}${hex(b)}${hex(a)}`;
}

/**
 * Read Ridge's terminal-relevant CSS variables and project them onto the
 * xterm.js-shaped key set the wasm kernel's `Theme::apply_partial` reads.
 * Only includes keys we successfully normalized — partial themes are fine,
 * apply_partial leaves unspecified palette entries alone.
 */
function readRidgeTheme(): Record<string, string> {
	if (typeof document === 'undefined') return {};
	const cs = getComputedStyle(document.documentElement);
	const v = (name: string) => normalizeColor(cs.getPropertyValue(name));

	const bg = v('--rg-term-bg');
	const fg = v('--rg-fg');
	const accent = v('--rg-accent');

	const out: Record<string, string> = {};
	if (bg) out.background = bg;
	if (fg) out.foreground = fg;
	if (accent) {
		out.cursor = accent;
		// Cursor-text-color (the glyph drawn ON TOP of the cursor block)
		// reads best as the bg color so it disappears into the cell.
		if (bg) out.cursorAccent = bg;
		// Hyperlink underline: same accent, full opacity.
		out.hyperlinkColor = accent;
		// Selection bg: accent tinted at ~24% alpha for readability.
		// Override with `--rg-selection-bg` if explicit control needed.
		const explicit = v('--rg-selection-bg');
		if (explicit) {
			out.selectionBackground = explicit;
		} else if (accent.length === 9) {
			// accent is "#rrggbbff" — swap alpha to 0x3d (~24%).
			out.selectionBackground = `${accent.slice(0, 7)}3d`;
		}
	}
	return out;
}

let _subscribed = false;
let _lastApplied: string | null = null;

/**
 * Wire CSS-variable → wasm-Theme propagation. Idempotent.
 *
 * Call once at app boot from `+page.svelte` onMount. Returns the
 * unsubscribe function; rarely needed (theme bridge lives for the
 * whole session in Ridge).
 */
export function setupTerminalThemeBridge(): () => void {
	if (_subscribed) return () => {};
	_subscribed = true;

	const manager = TerminalManager.instance();

	const push = () => {
		const theme = readRidgeTheme();
		// Skip if nothing changed since the last push — avoids walking
		// every pane's handle on unrelated settings updates (font size,
		// shell selection, …) that share the same store.
		const fingerprint = JSON.stringify(theme);
		if (fingerprint === _lastApplied) return;
		_lastApplied = fingerprint;
		manager.setTheme(theme);
	};

	// Initial push: the store fires immediately on subscribe. settings.ts's
	// `applyTheme` runs synchronously during `initSettingsBoot` so by the
	// time +page.svelte onMount fires the CSS vars are already correct.
	const unsubscribe = settingsStore.subscribe(() => {
		// settings.ts's setTheme calls applyTheme BEFORE persisting + fanning
		// the store update, so by the time the subscriber fires, the
		// `<html data-rg-theme>` attribute (and thus computed CSS vars)
		// reflect the new theme. Push synchronously.
		push();
	});

	return () => {
		unsubscribe();
		_subscribed = false;
	};
}

/** Force-push the current theme to the wasm kernel right now. Useful from
 *  test setups or after a manual CSS-var override. The store subscription
 *  in `setupTerminalThemeBridge` covers all normal paths. */
export function pushTerminalThemeNow(): void {
	const manager = TerminalManager.instance();
	const theme = readRidgeTheme();
	_lastApplied = JSON.stringify(theme);
	manager.setTheme(theme);
}
