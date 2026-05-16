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
import { termFontSize } from '$lib/stores/termSettings';
import { hex8 } from '$lib/utils/cssColor';
import { TerminalManager } from './manager';

// Color normalization moved to $lib/utils/cssColor — shared with
// $lib/monaco/ridgeTheme so wasm-kernel and Monaco editor parse the
// same way against the same `--rg-*` CSS-variable values.

/**
 * Read Ridge's terminal-relevant CSS variables and project them onto the
 * xterm.js-shaped key set the wasm kernel's `Theme::apply_partial` reads.
 * Only includes keys we successfully normalized — partial themes are fine,
 * apply_partial leaves unspecified palette entries alone.
 */
function readRidgeTheme(): Record<string, string> {
	if (typeof document === 'undefined') return {};
	const cs = getComputedStyle(document.documentElement);
	const v = (name: string) => hex8(cs.getPropertyValue(name));

	const bg = v('--rg-term-bg');
	const fg = v('--rg-fg');
	const accent = v('--rg-accent');
	const tuiBg = v('--rg-tui-bg');

	const out: Record<string, string> = {};
	if (bg) out.background = bg;
	if (fg) out.foreground = fg;
	if (tuiBg) out.tuiBackground = tuiBg;
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
		// Delay reading CSS vars by one frame — the browser needs a
		// paint cycle to compute new values after `data-rg-theme` changes.
		// Without this delay, getComputedStyle returns stale values.
		requestAnimationFrame(() => {
			const theme = readRidgeTheme();
			// Skip if nothing changed since the last push — avoids walking
			// every pane's handle on unrelated settings updates (font size,
			// shell selection, …) that share the same store.
			const fingerprint = JSON.stringify(theme);
			if (fingerprint !== _lastApplied) {
				_lastApplied = fingerprint;
				manager.setTheme(theme);
			}
		});
	};

	let _lastFontFamily: string | null = null;
	let _lastFontSize: number | null = null;

	const pushFont = (family: string, size: number) => {
		if (family === _lastFontFamily && size === _lastFontSize) return;
		_lastFontFamily = family;
		_lastFontSize = size;
		
		const resolvedFamily = family.trim() !== '' 
			? family 
			: "'JetBrains Mono','Cascadia Code','SF Mono',Consolas,ui-monospace,'Apple Color Emoji','Segoe UI Emoji','Noto Color Emoji',monospace";
		
		manager.setFont(resolvedFamily, size);
	};

	// Initial push: the store fires immediately on subscribe. settings.ts's
	// `applyTheme` runs synchronously during `initSettingsBoot` so by the
	// time +page.svelte onMount fires the CSS vars are already correct.
	const unsubscribeTheme = settingsStore.subscribe((settings) => {
		// settings.ts's setTheme calls applyTheme BEFORE persisting + fanning
		// the store update, so by the time the subscriber fires, the
		// `<html data-rg-theme>` attribute (and thus computed CSS vars)
		// reflect the new theme. Push synchronously.
		push();
		
		// Also sync font-family updates
		let size = _lastFontSize;
		// If termFontSize hasn't fired yet, try to read it now or fallback to 15
		if (size === null) {
			let currentSize = 15;
			termFontSize.subscribe(v => { currentSize = v; })();
			size = currentSize;
		}
		pushFont(settings.terminalFontFamily, size);
	});

	const unsubscribeFont = termFontSize.subscribe((size) => {
		let family = _lastFontFamily;
		if (family === null) {
			let currentSettings = { terminalFontFamily: '' };
			settingsStore.subscribe(v => { currentSettings = v; })();
			family = currentSettings.terminalFontFamily;
		}
		pushFont(family, size);
	});

	return () => {
		unsubscribeTheme();
		unsubscribeFont();
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
