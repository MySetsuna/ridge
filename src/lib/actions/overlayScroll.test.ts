/** overlayScroll action tests: use the action's exported presets and CSS layout branch. */
import { describe, expect, it, vi } from 'vitest';

const overlayInstance = vi.hoisted(() => ({ options: vi.fn(), destroy: vi.fn() }));
vi.mock('overlayscrollbars', () => ({
	OverlayScrollbars: vi.fn(() => overlayInstance),
}));

import { overlayScroll, PRESETS, PRESET_DEFAULT_LAYOUTS } from './overlayScroll';

function makeNode(): HTMLElement & { scrollLeft: number } {
	const classes = new Set<string>();
	return {
		style: {} as CSSStyleDeclaration,
		classList: {
			add: (...names: string[]) => names.forEach((name) => classes.add(name)),
			remove: (...names: string[]) => names.forEach((name) => classes.delete(name)),
			contains: (name: string) => classes.has(name),
		} as DOMTokenList,
		addEventListener: vi.fn(),
		removeEventListener: vi.fn(),
		scrollLeft: 0,
	} as unknown as HTMLElement & { scrollLeft: number };
}

describe('overlayScroll presets', () => {
	it('keeps sidebar vertical and horizontal-tabs native overflow contracts', () => {
		expect(PRESETS.sidebar.overflow).toMatchObject({ x: 'hidden', y: 'scroll' });
		expect(PRESETS['horizontal-tabs'].overflow).toMatchObject({ x: 'visible', y: 'hidden' });
	});

	it('enables drag/click scrolling and different auto-hide delays', () => {
		expect(PRESETS.sidebar.scrollbars).toMatchObject({ dragScroll: true, clickScroll: true, autoHideDelay: 600 });
		expect(PRESETS['horizontal-tabs'].scrollbars).toMatchObject({ dragScroll: true, clickScroll: true, autoHideDelay: 800 });
	});
});

describe('overlayScroll horizontal-tabs branch', () => {
	it('applies, updates, and destroys the native horizontal layout', () => {
		const node = makeNode();
		const action = overlayScroll(node, { preset: 'horizontal-tabs' });
		expect(node.style.display).toBe('flex');
		expect(node.style.flexDirection).toBe('row');
		expect(node.style.overflowX).toBe('auto');
		expect(node.classList.contains('rg-htabs')).toBe(true);
		expect(node.addEventListener).toHaveBeenCalledWith('wheel', expect.any(Function), { passive: false });

		action.update({ preset: 'horizontal-tabs' });
		action.destroy();
		expect(node.classList.contains('rg-htabs')).toBe(false);
		expect(node.removeEventListener).toHaveBeenCalledWith('wheel', expect.any(Function));
	});

	it('maps vertical wheel delta to horizontal scroll', () => {
		const node = makeNode();
		overlayScroll(node, { preset: 'horizontal-tabs' });
		const wheel = vi.mocked(node.addEventListener).mock.calls.find(([name]) => name === 'wheel')?.[1] as ((event: WheelEvent) => void) | undefined;
		expect(wheel).toBeDefined();
		const event = { deltaX: 0, deltaY: 24, preventDefault: vi.fn() } as unknown as WheelEvent;
		wheel?.(event);
		expect(event.preventDefault).toHaveBeenCalled();
		expect(node.scrollLeft).toBe(24);
	});
});

describe('overlayScroll sidebar branch', () => {
	it('applies custom layout/options and forwards update/destroy', () => {
		const node = makeNode();
		const action = overlayScroll(node, {
			preset: 'sidebar',
			layout: { direction: 'column', align: 'start', gap: 6 },
			options: { overflow: { y: 'hidden' } },
		});
		expect(node.style.flexDirection).toBe('column');
		expect(node.style.alignItems).toBe('start');
		expect(node.style.gap).toBe('6px');

		action.update({ preset: 'sidebar', layout: false });
		expect(overlayInstance.options).toHaveBeenCalled();
		action.destroy();
		expect(overlayInstance.destroy).toHaveBeenCalled();
	});
});

describe('overlayScroll default layouts', () => {
	it('horizontal-tabs defaults to row/center/4px', () => {
		expect(PRESET_DEFAULT_LAYOUTS['horizontal-tabs']).toEqual({ direction: 'row', align: 'center', gap: 4 });
		expect(PRESET_DEFAULT_LAYOUTS.sidebar).toBeUndefined();
	});
});
