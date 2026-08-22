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

import { hex8, hex8WithAlpha } from './cssColor';
import { TerminalManager } from './manager';
import { withEmojiFallback } from './fontStack';

// Color normalization moved to $lib/utils/cssColor — shared with
// $lib/monaco/ridgeTheme so wasm-kernel and Monaco editor parse the
// same way against the same `--rg-*` CSS-variable values.

/** Map a Ridge `--rg-ansi-*` CSS var name to its xterm.js key. */
const ANSI_CSS_TO_KEY: Record<string, string> = {
	'--rg-ansi-black': 'black',
	'--rg-ansi-red': 'red',
	'--rg-ansi-green': 'green',
	'--rg-ansi-yellow': 'yellow',
	'--rg-ansi-blue': 'blue',
	'--rg-ansi-magenta': 'magenta',
	'--rg-ansi-cyan': 'cyan',
	'--rg-ansi-white': 'white',
	'--rg-ansi-brightBlack': 'brightBlack',
	'--rg-ansi-brightRed': 'brightRed',
	'--rg-ansi-brightGreen': 'brightGreen',
	'--rg-ansi-brightYellow': 'brightYellow',
	'--rg-ansi-brightBlue': 'brightBlue',
	'--rg-ansi-brightMagenta': 'brightMagenta',
	'--rg-ansi-brightCyan': 'brightCyan',
	'--rg-ansi-brightWhite': 'brightWhite',
};

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

	const bgImageActive = TerminalManager.hostPorts()?.themes?.activeBgImageUrl() != null;

	const out: Record<string, string> = {};
	if (bg) {
		out.background = bgImageActive ? (hex8WithAlpha(bg, 0) ?? bg) : bg;
	}
	if (fg) out.foreground = fg;
	if (tuiBg) out.tuiBackground = tuiBg;
	if (accent) {
		out.cursor = accent;
		// Cursor-text-color (the glyph drawn ON TOP of the cursor block)
		// reads best as the bg color so it disappears into the cell. `bg` is
		// always the opaque `#rrggbbff` term-bg here (the alpha-0 form only
		// goes into out.background), so no extra normalization is needed —
		// even when a bg image is active the cursor glyph stays readable.
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

	// ANSI 16 colors: read `--rg-ansi-*` CSS vars set by the theme.
	// When a theme doesn't define ANSI overrides (dark themes keep their
	// wasm defaults), these CSS vars won't exist and hex8 returns null —
	// apply_partial simply leaves those palette entries untouched.
	for (const [cssVar, key] of Object.entries(ANSI_CSS_TO_KEY)) {
		const color = v(cssVar);
		if (color) out[key] = color;
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
	let pendingPushFrame: number | null = null;

	const push = () => {
		if (pendingPushFrame !== null) return;
		// Delay reading CSS vars by one frame — the browser needs a
		// paint cycle to compute new values after `data-rg-theme` changes.
		// Without this delay, getComputedStyle returns stale values.
		pendingPushFrame = requestAnimationFrame(() => {
			pendingPushFrame = null;
			const theme = readRidgeTheme();
			// Skip if nothing changed since the last push — avoids walking
			// every pane's handle on unrelated settings updates (font size,
			// shell selection, …) that share the same store.
			const fingerprint = JSON.stringify(theme);
			const changed = fingerprint !== _lastApplied;
			if (typeof localStorage !== 'undefined' && localStorage.getItem('RIDGE_THEME_TRACE') === '1') {
				const ts = performance.now().toFixed(1);
				const bg = theme.background ?? '∅';
				const fg = theme.foreground ?? '∅';
				const cur = theme.cursor ?? '∅';
				const sel = theme.selectionBackground ?? '∅';
				const ansiCount = Object.keys(theme).filter((k) => k.startsWith('bright') || ['black','red','green','yellow','blue','magenta','cyan','white'].includes(k)).length;
				// eslint-disable-next-line no-console
				console.debug(`[theme-trace][${ts}ms] push${changed ? '' : '/skip-unchanged'} bg=${bg} fg=${fg} cursor=${cur} sel=${sel} ansi=${ansiCount}/16`);
			}
			if (changed) {
				_lastApplied = fingerprint;
				manager.setTheme(theme);
			}
		});
	};

	let _lastFontFamily: string | null = null;
	let _lastFontSize: number | null = null;

	// Font ordering lives in ./fontStack. The native/browser font-data service
	// supplies the selected installed faces directly to the Wasm rasterizer.
	const pushFont = (family: string, size: number) => {
		if (family === _lastFontFamily && size === _lastFontSize) return;
		_lastFontFamily = family;
		_lastFontSize = size;

		void manager.setFont(withEmojiFallback(family), size).catch((error) => {
			console.error('[ridge-term] selected system font could not be loaded', error);
		});
	};

	// Initial push: the store fires immediately on subscribe. settings.ts's
	// `applyTheme` runs synchronously during `initSettingsBoot` so by the
	// time +page.svelte onMount fires the CSS vars are already correct.
	// §P2：settings/font/bg-image 的响应式来源经 HostPorts 注入（themeBridge 迁入
	// @ridge/remote 后不再直接 import 主 app store）。缺注入则订阅为 no-op，主题保持默认。
	const ports = TerminalManager.hostPorts();
	const unsubscribeTheme = ports?.settings?.subscribe((settings) => {
		// 由 hostPorts 包装 settingsStore.subscribe，Svelte 语义订阅即同步触发一次；
		// 此刻 CSS 变量已随 applyTheme 生效，可同步 push。
		push();
		let size = _lastFontSize;
		size ??= ports?.termSettings?.fontSize() ?? 15;
		pushFont(settings.terminalFontFamily, size);
	}) ?? (() => {});

	const unsubscribeFont = ports?.termSettings?.subscribe((size) => {
		let family = _lastFontFamily;
		family ??= ports?.settings?.get().terminalFontFamily ?? '';
		pushFont(family, size);
	}) ?? (() => {});

	const unsubscribeBgImage = ports?.themes?.subscribe(() => {
		push();
	}) ?? (() => {});

	return () => {
		if (pendingPushFrame !== null) cancelAnimationFrame(pendingPushFrame);
		pendingPushFrame = null;
		unsubscribeTheme();
		unsubscribeFont();
		unsubscribeBgImage();
		_subscribed = false;
	};
}

/** Force-push the current theme to the wasm kernel right now. Useful from
 *  test setups, after a manual CSS-var override, or from a freshly-attached
 *  pane that wants the kernel rebased on the current theme without
 *  waiting for the bridge's next RAF.
 *
 *  Important: bails out silently when `readRidgeTheme()` returns an empty
 *  object — that happens before `initSettingsBoot()` writes any `--rg-*`
 *  CSS vars onto documentElement, e.g. when a pane's onMount finishes
 *  ahead of `+page.svelte`'s async theme-bootstrap IIFE. Pushing the
 *  empty theme would `setTheme({})` → manager.setTheme calls
 *  `handle.applyDefaultTheme()` on every pane, which rebases the wasm
 *  kernel's `Theme::bg` back to `default_dark` (`#071009`, "near-black
 *  dark green"). The kernel would then visibly flash to the default-dark
 *  palette and only recover on the bridge's next RAF — exactly the
 *  "background should be theme color but is black" symptom. Leaving the
 *  existing `opts.theme` intact lets the bridge's pending RAF (already
 *  scheduled at boot via `setupTerminalThemeBridge` subscribing to
 *  settingsStore) apply the right theme as soon as CSS vars land. */
export function pushTerminalThemeNow(): void {
	const manager = TerminalManager.instance();
	const theme = readRidgeTheme();
	// `background` is the only field always set when CSS vars are
	// populated (everything else may be absent if a theme didn't
	// declare e.g. `selectionBackground`). If `background` is empty,
	// the whole probe missed — defer to the bridge's RAF rather than
	// blow away an already-applied theme.
	if (!theme.background) return;
	_lastApplied = JSON.stringify(theme);
	manager.setTheme(theme);
}
