// src/lib/monaco/ridgeTheme.ts
//
// Make Monaco's editor chrome track Ridge's theme. For each Ridge theme
// id (`dark` / `sand` / `grass` / `soil` / `wheat` / `starsky`) we
// register a custom Monaco theme `ridge-${themeId}` that:
//
//   - inherits from `vs` (light Ridge themes) or `vs-dark` (dark) so
//     syntax token colors carry over without a per-language rewrite,
//   - overrides editor.background / foreground / selection / cursor /
//     line-number / minimap / scrollbar / widget chrome with the
//     current `--rg-*` CSS variable values.
//
// `applyRidgeMonacoTheme` is idempotent: calling it again with the
// same id re-reads CSS vars and re-defines the theme (Monaco's
// defineTheme overwrites by id), then setTheme retints both the inline
// editor and any active diff editor.

import * as monaco from 'monaco-editor';
import { hex8, hex8WithAlpha } from '$lib/utils/cssColor';
// ThemeId is now a plain string — themes are loaded dynamically from ridge.theme

/**
 * Ridge themes whose `--rg-bg` is dark enough that Monaco's `vs-dark`
 * base produces better syntax-token contrast. Light Ridge themes use
 * `vs`. Authoritative source: `src/app.css:17-150` palette blocks.
 */
export const THEMES_DARK: ReadonlySet<string> = new Set<string>([
	'endless-dark',
	'dark',
	'soil',
	'starsky',
]);

const FALLBACK_DARK = '#1e1e1e';
const FALLBACK_LIGHT = '#ffffff';
const FALLBACK_FG_DARK = '#d4d4d4';
const FALLBACK_FG_LIGHT = '#000000';

/** Build the Monaco theme id used when registering / setting a theme. */
export function ridgeMonacoThemeId(themeId: string): string {
	return `ridge-${themeId}`;
}

/**
 * Read the live `--rg-*` CSS variable values, build a Monaco theme
 * from them, register it under `ridge-${themeId}`, and apply it.
 *
 * Safe to call before Monaco mounts — it just registers the theme and
 * `monaco.editor.setTheme` propagates to any editors that exist or
 * will be created with the same id.
 *
 * In SSR / pre-hydration contexts where `getComputedStyle` is
 * unavailable, falls back to plain `vs` / `vs-dark` so the editor
 * still renders something reasonable until the next theme application.
 */
export function applyRidgeMonacoTheme(themeId: string): void {
	const isDark = THEMES_DARK.has(themeId);
	const monacoId = ridgeMonacoThemeId(themeId);

	if (typeof document === 'undefined') {
		monaco.editor.setTheme(isDark ? 'vs-dark' : 'vs');
		return;
	}

	const cs = getComputedStyle(document.documentElement);
	const cssVar = (name: string): string => cs.getPropertyValue(name).trim();

	const bgRaw = cssVar('--rg-bg');
	const bgRaisedRaw = cssVar('--rg-bg-raised');
	const surfaceRaw = cssVar('--rg-surface');
	const fgRaw = cssVar('--rg-fg');
	const fgMutedRaw = cssVar('--rg-fg-muted');
	const accentRaw = cssVar('--rg-accent');
	const borderRaw = cssVar('--rg-border-bright');

	const bg = hex8(bgRaw) ?? (isDark ? `${FALLBACK_DARK}ff` : `${FALLBACK_LIGHT}ff`);
	const bgRaised = hex8(bgRaisedRaw) ?? bg;
	const surface = hex8(surfaceRaw) ?? bgRaised;
	const fg =
		hex8(fgRaw) ?? (isDark ? `${FALLBACK_FG_DARK}ff` : `${FALLBACK_FG_LIGHT}ff`);
	const fgMuted = hex8(fgMutedRaw) ?? fg;
	const accent = hex8(accentRaw) ?? fg;
	const border = hex8(borderRaw) ?? hex8WithAlpha(fg, 0.16) ?? fg;

	const accent30 = hex8WithAlpha(accentRaw, 0.3) ?? accent;
	const accent18 = hex8WithAlpha(accentRaw, 0.18) ?? accent;
	const accent40 = hex8WithAlpha(accentRaw, 0.4) ?? accent;
	const accent20 = hex8WithAlpha(accentRaw, 0.2) ?? accent;
	const accent50 = hex8WithAlpha(accentRaw, 0.5) ?? accent;
	const fgMuted20 = hex8WithAlpha(fgMutedRaw, 0.2) ?? fgMuted;
	const fgMuted35 = hex8WithAlpha(fgMutedRaw, 0.35) ?? fgMuted;

	monaco.editor.defineTheme(monacoId, {
		base: isDark ? 'vs-dark' : 'vs',
		inherit: true,
		rules: [],
		colors: {
			'editor.background': bg,
			'editor.foreground': fg,
			'editorCursor.foreground': accent,
			'editorLineNumber.foreground': fgMuted,
			'editorLineNumber.activeForeground': fg,
			'editor.lineHighlightBackground': bgRaised,
			'editor.selectionBackground': accent30,
			'editor.inactiveSelectionBackground': accent18,
			'editor.findMatchBackground': accent40,
			'editor.findMatchHighlightBackground': accent20,
			'editorIndentGuide.background': border,
			'editorIndentGuide.activeBackground': accent40,
			'editorWidget.background': surface,
			'editorWidget.border': border,
			'editorGutter.background': bg,
			'minimap.background': bgRaised,
			'scrollbarSlider.background': fgMuted20,
			'scrollbarSlider.hoverBackground': fgMuted35,
			'scrollbarSlider.activeBackground': accent50,
		},
	});
	monaco.editor.setTheme(monacoId);
}
