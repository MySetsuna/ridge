import { afterEach, describe, expect, it, vi } from 'vitest';

const monacoSpies = vi.hoisted(() => ({
	defineTheme: vi.fn(),
	setTheme: vi.fn(),
}));
const cssColor = vi.hoisted(() => ({
	hex8: vi.fn((value: string) => {
		const colors: Record<string, string> = {
			'#101010': '#101010ff',
			'#202020': '#202020ff',
			'#303030': '#303030ff',
			'#f0f0f0': '#f0f0f0ff',
			'#a0a0a0': '#a0a0a0ff',
			'#ff9900': '#ff9900ff',
			'#505050': '#505050ff',
		};
		return colors[value] ?? null;
	}),
	hex8WithAlpha: vi.fn((value: string, alpha: number) =>
		value ? `${value}@${alpha}` : null
	),
}));

vi.mock('monaco-editor', () => ({ editor: monacoSpies }));
vi.mock('@ridge/remote/shared/terminal/cssColor', () => cssColor);

import { applyRidgeMonacoTheme, ridgeMonacoThemeId, THEMES_DARK } from './ridgeTheme';

afterEach(() => {
	monacoSpies.defineTheme.mockReset();
	monacoSpies.setTheme.mockReset();
	cssColor.hex8.mockClear();
	cssColor.hex8WithAlpha.mockClear();
	Reflect.deleteProperty(globalThis, 'document');
	Reflect.deleteProperty(globalThis, 'getComputedStyle');
});

describe('ridge Monaco theme projection', () => {
	it('builds stable ids and classifies dark themes', () => {
		expect(ridgeMonacoThemeId('soil')).toBe('ridge-soil');
		expect(THEMES_DARK.has('soil')).toBe(true);
		expect(THEMES_DARK.has('sand')).toBe(false);
	});

	it('uses the base Monaco theme during SSR', () => {
		applyRidgeMonacoTheme('dark');
		applyRidgeMonacoTheme('sand');

		expect(monacoSpies.defineTheme).not.toHaveBeenCalled();
		expect(monacoSpies.setTheme).toHaveBeenNthCalledWith(1, 'vs-dark');
		expect(monacoSpies.setTheme).toHaveBeenNthCalledWith(2, 'vs');
	});

	it('registers CSS colors and applies the custom browser theme', () => {
		const values: Record<string, string> = {
			'--rg-bg': '#101010',
			'--rg-bg-raised': '#202020',
			'--rg-surface': '#303030',
			'--rg-fg': '#f0f0f0',
			'--rg-fg-muted': '#a0a0a0',
			'--rg-accent': '#ff9900',
			'--rg-border-bright': '#505050',
		};
		Object.defineProperty(globalThis, 'document', {
			configurable: true,
			value: { documentElement: {} },
		});
		Object.defineProperty(globalThis, 'getComputedStyle', {
			configurable: true,
			value: () => ({ getPropertyValue: (name: string) => values[name] ?? '' }),
		});

		applyRidgeMonacoTheme('dark');

		expect(monacoSpies.defineTheme).toHaveBeenCalledWith('ridge-dark', expect.objectContaining({
			base: 'vs-dark',
			inherit: true,
			colors: expect.objectContaining({
				'editor.background': '#101010ff',
				'editor.foreground': '#f0f0f0ff',
				'editorCursor.foreground': '#ff9900ff',
				'editorWidget.background': '#303030ff',
				'editorWidget.border': '#505050ff',
			}),
		}));
		expect(monacoSpies.setTheme).toHaveBeenCalledWith('ridge-dark');
	});

	it('falls back to a light palette when CSS values are unavailable', () => {
		Object.defineProperty(globalThis, 'document', {
			configurable: true,
			value: { documentElement: {} },
		});
		Object.defineProperty(globalThis, 'getComputedStyle', {
			configurable: true,
			value: () => ({ getPropertyValue: () => '' }),
		});

		applyRidgeMonacoTheme('sand');

		const theme = monacoSpies.defineTheme.mock.calls[0][1] as { base: string; colors: Record<string, string> };
		expect(theme.base).toBe('vs');
		expect(theme.colors['editor.background']).toBe('#ffffffff');
		expect(theme.colors['editor.foreground']).toBe('#000000ff');
		expect(monacoSpies.setTheme).toHaveBeenCalledWith('ridge-sand');
	});
});
