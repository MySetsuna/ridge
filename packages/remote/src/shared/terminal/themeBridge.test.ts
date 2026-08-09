import { afterEach, describe, expect, it, vi } from 'vitest';

const { manager, ports, ensureFlagFont, withEmojiFallback } = vi.hoisted(() => ({
	manager: { setTheme: vi.fn(), setFont: vi.fn() },
	ports: {
		settings: {
			subscribe: (cb: (value: { terminalFontFamily: string }) => void) => { cb({ terminalFontFamily: 'Mono' }); return vi.fn(); },
			get: () => ({ terminalFontFamily: 'Mono' }),
		},
		termSettings: {
			subscribe: (cb: (value: number) => void) => { cb(15); return vi.fn(); },
			fontSize: () => 15,
		},
		themes: {
			subscribe: (cb: () => void) => { void cb; return vi.fn(); },
			activeBgImageUrl: () => null,
		},
	},
	ensureFlagFont: vi.fn(() => false),
	withEmojiFallback: vi.fn((family: string) => `normalized:${family}`),
}));

vi.mock('./manager', () => ({
	TerminalManager: {
		instance: () => manager,
		hostPorts: () => ports,
	},
}));
vi.mock('./flagEmojiSupport', () => ({ ensureFlagFont }));
vi.mock('./fontStack', () => ({ withEmojiFallback }));

import { pushTerminalThemeNow, setupTerminalThemeBridge } from './themeBridge';

let cssValues: Record<string, string> = {};
let frames: FrameRequestCallback[] = [];

function installDom() {
	const canvasContext = {
		fillStyle: '#000000',
	};
	vi.stubGlobal('document', {
		documentElement: {},
		createElement: vi.fn(() => ({ getContext: () => canvasContext })),
	});
	vi.stubGlobal('getComputedStyle', () => ({ getPropertyValue: (name: string) => cssValues[name] ?? '' }));
	vi.stubGlobal('requestAnimationFrame', (cb: FrameRequestCallback) => { frames.push(cb); return frames.length; });
	vi.stubGlobal('cancelAnimationFrame', vi.fn());
	vi.stubGlobal('localStorage', { getItem: vi.fn(() => null) });
	vi.stubGlobal('performance', { now: () => 1 });
}

afterEach(() => {
	cssValues = {};
	frames = [];
	manager.setTheme.mockClear();
	manager.setFont.mockClear();
	ports.themes.activeBgImageUrl = () => null;
	vi.unstubAllGlobals();
});

describe('terminal theme bridge', () => {
	it('does not reset the kernel when CSS variables are unavailable', () => {
		installDom();
		pushTerminalThemeNow();
		expect(manager.setTheme).not.toHaveBeenCalled();
	});

	it('normalizes Ridge variables, ANSI colors, selection alpha, and image backgrounds', () => {
		installDom();
		cssValues = {
			'--rg-term-bg': '#112233',
			'--rg-fg': '#ddeeff',
			'--rg-accent': '#aabbcc',
			'--rg-tui-bg': '#010203',
			'--rg-ansi-red': '#ff0000',
		};
		pushTerminalThemeNow();
		expect(manager.setTheme).toHaveBeenLastCalledWith(expect.objectContaining({
			background: '#112233ff',
			foreground: '#ddeeffff',
			cursor: '#aabbccff',
			cursorAccent: '#112233ff',
			selectionBackground: '#aabbcc3d',
			red: '#ff0000ff',
		}));
	});

	it('coalesces setup pushes and propagates font settings', () => {
		installDom();
		cssValues = { '--rg-term-bg': '#111111', '--rg-fg': '#eeeeee' };
		const stop = setupTerminalThemeBridge();
		expect(ensureFlagFont).toHaveBeenCalled();
		expect(withEmojiFallback).toHaveBeenCalledWith('Mono', false);
		expect(manager.setFont).toHaveBeenCalledWith('normalized:Mono', 15);
		expect(frames).toHaveLength(1);
		frames.shift()?.(0);
		expect(manager.setTheme).toHaveBeenCalledWith(expect.objectContaining({ background: '#111111ff' }));
		stop();
	});
});
