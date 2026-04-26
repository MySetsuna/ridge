// src/lib/actions/overlayScroll.ts
//
// Svelte 5 action wrapper around the `overlayscrollbars` library.
// Gives sidebar/explorer/SCM regions a VS Code-style overlay scrollbar.
//
// Usage:
// <div use:overlayScroll>...</div> // 'sidebar' preset (default, uses OverlayScrollbars)
// <div use:overlayScroll={{ preset: 'horizontal-tabs' }}> // horizontal scroll (CSS native, no OverlayScrollbars)
//
// For 'horizontal-tabs': uses pure CSS horizontal scrolling to avoid OverlayScrollbars
// viewport manipulation issues that prevent horizontal tabs from working correctly.

import { OverlayScrollbars, type PartialOptions } from 'overlayscrollbars';
import 'overlayscrollbars/overlayscrollbars.css';

/** Preset names */
export type OverlayScrollPreset = 'sidebar' | 'horizontal-tabs';

/** Inline flex layout applied to the host element by the action. */
export interface OverlayScrollLayout {
	direction?: 'row' | 'column';
	align?: 'start' | 'center' | 'end' | 'stretch';
	gap?: number | string;
}

export interface OverlayScrollOptions {
	preset?: OverlayScrollPreset;
	options?: PartialOptions;
	layout?: OverlayScrollLayout | false;
}

/** Sidebar: uses OverlayScrollbars for vertical scroll */
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
		// Empty - will use pure CSS native scrolling
		scrollbars: {
			theme: 'wf-os-theme',
			autoHide: 'leave',
			autoHideDelay: 800,
			dragScroll: true,
			clickScroll: true,
		},
		overflow: {
			x: 'visible',
			y: 'hidden',
		},
	},
};

const PRESET_DEFAULT_LAYOUTS: Partial<Record<OverlayScrollPreset, OverlayScrollLayout>> = {
	'horizontal-tabs': { direction: 'row', align: 'center', gap: 4 },
};

function mergeOptions(preset: PartialOptions, override: PartialOptions | undefined): PartialOptions {
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

const LAYOUT_PROPS = ['display', 'flexDirection', 'flexWrap', 'alignItems', 'gap', 'width', 'minWidth', 'overflowX', 'overflowY'] as const;

function applyLayout(node: HTMLElement, params: OverlayScrollOptions | undefined): void {
	for (const k of LAYOUT_PROPS) node.style[k] = '';

	const preset = params?.preset ?? 'sidebar';

	// For horizontal-tabs, apply pure CSS horizontal scrolling
	if (preset === 'horizontal-tabs') {
		node.style.display = 'flex';
		node.style.flexDirection = 'row';
		node.style.flexWrap = 'nowrap';
		node.style.alignItems = 'center';
		node.style.gap = '4px';
		node.style.overflowX = 'auto';
		node.style.overflowY = 'hidden';
		node.style.width = 'max-content';
		node.style.minWidth = 'max-content';
		return;
	}

	// For sidebar, apply layout from preset
	const layout: OverlayScrollLayout | false | undefined =
		params?.layout !== undefined
			? params.layout
			: params?.preset
			? PRESET_DEFAULT_LAYOUTS[params.preset]
			: undefined;

	if (!layout) return;

	node.style.display = 'flex';
	node.style.flexDirection = layout.direction ?? 'row';
	node.style.flexWrap = 'nowrap';
	node.style.alignItems = layout.align ?? 'center';
	if (layout.gap !== undefined) {
		node.style.gap = typeof layout.gap === 'number' ? `${layout.gap}px` : layout.gap;
	}
}

export function overlayScroll(
	node: HTMLElement,
	params: OverlayScrollOptions | undefined = undefined
) {
	const preset = params?.preset ?? 'sidebar';

	// For horizontal-tabs: use pure CSS, no OverlayScrollbars
	if (preset === 'horizontal-tabs') {
		applyLayout(node, params);

		return {
			update(next: OverlayScrollOptions | undefined) {
				applyLayout(node, next);
			},
			destroy() {
				for (const k of LAYOUT_PROPS) node.style[k] = '';
			},
		};
	}

	// For sidebar: use OverlayScrollbars
	applyLayout(node, params);
	const instance = OverlayScrollbars(node, resolveOptions(params));

	return {
		update(next: OverlayScrollOptions | undefined) {
			applyLayout(node, next);
			instance.options(resolveOptions(next));
		},
		destroy() {
			for (const k of LAYOUT_PROPS) node.style[k] = '';
			instance.destroy();
		},
	};
}