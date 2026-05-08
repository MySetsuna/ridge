// src/lib/actions/overlayScroll.ts
//
// Svelte 5 action wrapper around the `overlayscrollbars` library.
// Gives sidebar/explorer/SCM regions a VS Code-style overlay scrollbar.
//
// Usage:
// <div use:overlayScroll>...</div> // 'sidebar' preset (default, uses OverlayScrollbars)
// <div use:overlayScroll={{ preset: 'horizontal-tabs' }}> // horizontal scroll (CSS native)
//
// For 'horizontal-tabs': pure CSS overflow-x:auto with the host bounded by its parent.
// Wheel events are intercepted so vertical scroll → horizontal pan (no Shift needed).
// Scrollbar is hidden via .rg-htabs CSS class (app.css).

import { OverlayScrollbars, type PartialOptions } from 'overlayscrollbars';
import 'overlayscrollbars/overlayscrollbars.css';

/**
 * One-time global guard: while the user is dragging a scrollbar handle or
 * track, toggle `body.rg-os-dragging` so app.css can suppress text selection
 * across the whole document. overlayscrollbars handles pointer routing but
 * does not stop the browser from extending a pre-existing selection during
 * the drag, nor shield text adjacent to the handle on jittery pointerdowns.
 * The class is consumed by the `body.rg-os-dragging *` rule in app.css.
 */
let dragGuardInstalled = false;
function installScrollbarDragGuard(): void {
  if (dragGuardInstalled || typeof document === 'undefined') return;
  dragGuardInstalled = true;
  const onDown = (e: PointerEvent): void => {
    const target = e.target as Element | null;
    if (target?.closest('.os-scrollbar-handle, .os-scrollbar-track')) {
      document.body.classList.add('rg-os-dragging');
    }
  };
  const clear = (): void => {
    document.body.classList.remove('rg-os-dragging');
  };
  document.addEventListener('pointerdown', onDown, true);
  document.addEventListener('pointerup', clear, true);
  document.addEventListener('pointercancel', clear, true);
  // Window blur (alt-tab during drag) — release lock so selection works again.
  window.addEventListener('blur', clear);
}

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
			theme: 'rg-os-theme',
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
			theme: 'rg-os-theme',
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

function applyHTabsLayout(node: HTMLElement): void {
	// Only set scroll/overflow properties — do NOT set width/minWidth.
	// The host is bounded by its flex parent; children overflow it horizontally.
	node.style.display = 'flex';
	node.style.flexDirection = 'row';
	node.style.flexWrap = 'nowrap';
	node.style.alignItems = 'center';
	node.style.overflowX = 'auto';
	node.style.overflowY = 'hidden';
}

function applyLayout(node: HTMLElement, params: OverlayScrollOptions | undefined): void {
	for (const k of LAYOUT_PROPS) node.style[k] = '';

	const preset = params?.preset ?? 'sidebar';

	if (preset === 'horizontal-tabs') {
		applyHTabsLayout(node);
		return;
	}

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
	installScrollbarDragGuard();
	const preset = params?.preset ?? 'sidebar';

	// For horizontal-tabs: use pure CSS, no OverlayScrollbars
	if (preset === 'horizontal-tabs') {
		applyHTabsLayout(node);
		node.classList.add('rg-htabs');

		// Intercept wheel events: vertical scroll → horizontal pan (no Shift needed).
		// passive:false required so preventDefault() actually stops native page scroll.
		const onWheel = (e: WheelEvent) => {
			e.preventDefault();
			// Prefer native horizontal delta (trackpad); fall back to vertical.
			node.scrollLeft += e.deltaX !== 0 ? e.deltaX : e.deltaY;
		};
		node.addEventListener('wheel', onWheel, { passive: false });

		return {
			update(next: OverlayScrollOptions | undefined) {
				applyHTabsLayout(node);
			},
			destroy() {
				for (const k of LAYOUT_PROPS) node.style[k] = '';
				node.classList.remove('rg-htabs');
				node.removeEventListener('wheel', onWheel);
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
