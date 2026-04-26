// src/lib/actions/overlayScroll.ts
//
// Svelte 5 action wrapper around the `overlayscrollbars` library.
// Gives sidebar/explorer/SCM regions a VS Code-style overlay scrollbar:
// - scrollbar floats on top of content (no gutter reservation, no resize on show/hide)
// - hides after idle, re-appears on scroll/hover
// - smooth wheel handling, deterministic on Windows WebView2
//
// Usage:
// <div use:overlayScroll>...</div> // 'sidebar' preset (default)
// <div use:overlayScroll={{ preset: 'horizontal-tabs' }}> // horizontal flex + scroll
// <div use:overlayScroll={{ preset: 'horizontal-tabs', layout: { gap: 8 } }}> // custom gap
// <div use:overlayScroll={{ options: {...} }}> // raw override
//
// Layout injection (horizontal-tabs preset):
// The `horizontal-tabs` preset automatically applies `display:flex; flex-direction:row;
// align-items:center; gap:4px` to the host so consumers don't repeat flex utility classes.
// Override any key via the `layout` option, or pass `layout: false` to opt out entirely.

import { OverlayScrollbars, type PartialOptions } from 'overlayscrollbars';
// eslint-disable-next-line import/no-unresolved -- bundler resolves the ./overlayscrollbars.css asset inside the package
import 'overlayscrollbars/overlayscrollbars.css';

/**
 * Preset names — keep this union small and documented. Each string maps
 * to a baseline `PartialOptions` block in `PRESETS` below.
 */
export type OverlayScrollPreset = 'sidebar' | 'horizontal-tabs';

/** Inline flex layout applied to the host element by the action. */
export interface OverlayScrollLayout {
	/** flex-direction. Default: 'row'. */
	direction?: 'row' | 'column';
	/** align-items. Default: 'center'. */
	align?: 'start' | 'center' | 'end' | 'stretch';
	/** gap between children. Number = px, string = any CSS value. Default: 4. */
	gap?: number | string;
}

export interface OverlayScrollOptions {
	/** Preset to base the config on. Defaults to `'sidebar'`. */
	preset?: OverlayScrollPreset;
	/** Additional options merged on top of the preset (shallow per-key). */
	options?: PartialOptions;
	/**
	 * Inline flex layout applied to the host after the scrollbars instance is
	 * created. Defaults to the preset's built-in layout (if any). Pass `false`
	 * to suppress the preset default and keep the host's existing layout.
	 */
	layout?: OverlayScrollLayout | false;
}

/**
 * `sidebar` — vertical scroll only, x hidden. The original default. Used
 * by Explorer, SCM, Search, ScrollbackHistoryModal, Markdown preview.
 *
 * `horizontal-tabs` — horizontal scroll only, y hidden, autoHide=`leave`
 * (visible on hover, hidden when pointer leaves). Also injects flex-row
 * layout onto the host so tab children line up with gaps.
 */
const PRESETS: Record<OverlayScrollPreset, PartialOptions> = {
	sidebar: {
		scrollbars: {
			theme: 'wf-os-theme',
			autoHide: 'leave',
			autoHideDelay: 600,
			dragScroll: true,
			clickScroll: true,
		},
		overflow: {
			x: 'hidden',
			y: 'scroll',
		},
	},
	'horizontal-tabs': {
		scrollbars: {
			theme: 'wf-os-theme',
			autoHide: 'leave',
			autoHideDelay: 800,
			dragScroll: true,
			clickScroll: true,
		},
		overflow: {
			x: 'scroll',
			y: 'hidden',
		},
	},
};

/** Default flex layout per preset. The `sidebar` preset has no layout (it's a
 * block container). `horizontal-tabs` gets row flex + center-align + 4px gap. */
const PRESET_DEFAULT_LAYOUTS: Partial<Record<OverlayScrollPreset, OverlayScrollLayout>> = {
	'horizontal-tabs': { direction: 'row', align: 'center', gap: 4 },
};

/** Deep-merge a preset with caller overrides. Only nested keys
 * (`scrollbars`, `overflow`) get spread; everything else is plain spread.
 * Keeps the merge predictable so callers can override one knob without
 * losing the rest of the preset. */
function mergeOptions(
	preset: PartialOptions,
	override: PartialOptions | undefined
): PartialOptions {
	if (!override) return preset;
	return {
		...preset,
		...override,
		scrollbars: {
			...(preset.scrollbars ?? {}),
			...(override.scrollbars ?? {}),
		},
		overflow: {
			...(preset.overflow ?? {}),
			...(override.overflow ?? {}),
		},
	};
}

function resolveOptions(params: OverlayScrollOptions | undefined): PartialOptions {
	const preset = PRESETS[params?.preset ?? 'sidebar'];
	return mergeOptions(preset, params?.options);
}

/** CSS properties we inject onto the HOST element — tracked so destroy() can clean them up. */
const LAYOUT_PROPS = ['display', 'flexDirection', 'alignItems', 'gap'] as const;

function applyLayout(node: HTMLElement, params: OverlayScrollOptions | undefined): void {
	// Clear any values we previously stamped.
	for (const k of LAYOUT_PROPS) node.style[k] = '';

	// Resolve: explicit layout > preset default > nothing.
	const layout: OverlayScrollLayout | false | undefined =
		params?.layout !== undefined
			? params.layout
			: params?.preset
				? PRESET_DEFAULT_LAYOUTS[params.preset]
				: undefined;

	if (!layout) return;

	node.style.display = 'flex';
	node.style.flexDirection = layout.direction ?? 'row';
	node.style.alignItems = layout.align ?? 'center';
	if (layout.gap !== undefined) {
		node.style.gap = typeof layout.gap === 'number' ? `${layout.gap}px` : layout.gap;
	}
}

/**
 * Apply flex layout to the `.os-content` element created by overlayscrollbars.
 *
 * overlayscrollbars wraps the host's children into `.os-viewport > .os-content`,
 * so any flex layout on the HOST element has no effect on the tab children.
 * This function targets `.os-content` directly, which is where the tabs live.
 *
 * Only relevant for `horizontal-tabs` preset where tabs need to flow in a row.
 */
function applyContentLayout(node: HTMLElement, params: OverlayScrollOptions | undefined): void {
	const preset = params?.preset ?? 'sidebar';
	if (preset !== 'horizontal-tabs') return;

	const contentEl = node.querySelector<HTMLElement>('.os-content');
	if (!contentEl) {
		// Retry once after a microtask - OverlayScrollbars may create .os-content asynchronously
		queueMicrotask(() => {
			const retryEl = node.querySelector<HTMLElement>('.os-content');
			if (retryEl) {
				applyContentLayoutImpl(retryEl, params);
			}
		});
		return;
	}
	applyContentLayoutImpl(contentEl, params);
}

function applyContentLayoutImpl(
	contentEl: HTMLElement,
	params: OverlayScrollOptions | undefined
): void {
	// Always apply row flex to content for horizontal-tabs regardless of host layout.
	// `layout: false` on params means "don't touch the host" - content still needs flex.
	const explicitLayout = params?.layout;
	const layout: OverlayScrollLayout =
		explicitLayout !== false && explicitLayout !== undefined
			? explicitLayout
			: (PRESET_DEFAULT_LAYOUTS['horizontal-tabs'] ?? { direction: 'row', align: 'center', gap: 4 });

	contentEl.style.display = 'flex';
	contentEl.style.flexDirection = layout.direction ?? 'row';
	contentEl.style.alignItems = layout.align ?? 'center';
	if (layout.gap !== undefined) {
		contentEl.style.gap = typeof layout.gap === 'number' ? `${layout.gap}px` : layout.gap;
	}
	// tabs must not wrap, and the content block must grow past the viewport to trigger overflow
	contentEl.style.whiteSpace = 'nowrap';
	contentEl.style.minWidth = 'max-content';
}

export function overlayScroll(
	node: HTMLElement,
	params: OverlayScrollOptions | undefined = undefined
) {
	applyLayout(node, params);
	const instance = OverlayScrollbars(node, resolveOptions(params));
	// Must run AFTER OverlayScrollbars() since it creates .os-content during init.
	applyContentLayout(node, params);

	return {
		update(next: OverlayScrollOptions | undefined) {
			applyLayout(node, next);
			instance.options(resolveOptions(next));
			applyContentLayout(node, next);
		},
		destroy() {
			for (const k of LAYOUT_PROPS) node.style[k] = '';
			instance.destroy();
		},
	};
}