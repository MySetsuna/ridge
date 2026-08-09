import { beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

const invokeMock = vi.hoisted(() => vi.fn());
const convertFileSrcMock = vi.hoisted(() => vi.fn((path: string) => `asset://${path}`));

vi.mock('@tauri-apps/api/core', () => ({
	invoke: invokeMock,
	convertFileSrc: convertFileSrcMock,
}));

import {
	activeBgImage,
	constrainWallpaperSize,
	deleteCustomTheme,
	getTheme,
	getThemeIds,
	getThemeLabels,
	initThemeSystem,
	isCustomTheme,
	refreshThemes,
	resolveThemeBgUrl,
	saveCustomTheme,
	saveThemeBgImage,
	saveThemeBgImageFromPath,
	setActiveBgImage,
	slugifyThemeId,
	type ThemeEntry,
} from './themes';

const theme = (overrides: Partial<ThemeEntry> = {}): ThemeEntry => ({
	id: 'endless-dark',
	label: 'Endless Dark',
	type: 'dark',
	loader: { primary: '#fff', secondary: '#000' },
	colors: { background: '#071009' },
	...overrides,
});

beforeEach(() => {
	invokeMock.mockReset();
	convertFileSrcMock.mockClear();
});

describe('theme data and bounded image helpers', () => {
	it('slugifies custom labels and identifies editable themes', () => {
		expect(slugifyThemeId('  Hello, Ridge 5! ')).toBe('custom-hello-ridge-5');
		expect(slugifyThemeId('---')).toBe('custom-theme');
		expect(isCustomTheme('custom-hello')).toBe(true);
		expect(isCustomTheme('endless-dark')).toBe(false);
	});

	it('constrains invalid and oversized wallpaper dimensions', () => {
		expect(constrainWallpaperSize(0, 20)).toEqual({ width: 0, height: 0 });
		expect(constrainWallpaperSize(Number.NaN, 20)).toEqual({ width: 0, height: 0 });
		expect(constrainWallpaperSize(10000, 5000)).toEqual({ width: 4096, height: 2048 });
		expect(constrainWallpaperSize(640, 480)).toEqual({ width: 640, height: 480 });
	});

	it('loads theme data, exposes stable getters, and resolves asset URLs', async () => {
		const loaded = theme({ id: 'custom-ridge', label: 'Ridge' });
		invokeMock.mockImplementation(async (command: string) => {
			if (command === 'get_theme_data') return { version: 1, themes: [loaded] };
			if (command === 'get_theme_assets_dir') return 'C:\\theme-assets\\';
			throw new Error(`unexpected command ${command}`);
		});
		await initThemeSystem();
		expect(getThemeIds()).toEqual(['custom-ridge']);
		expect(getThemeLabels()).toEqual({ 'custom-ridge': 'Ridge' });
		expect(getTheme('custom-ridge')).toEqual(loaded);
		expect(await resolveThemeBgUrl(undefined)).toBeNull();
		expect(await resolveThemeBgUrl(theme({ bgImage: 'wall.png' }))).toBe('asset://C:\\theme-assets\\wall.png');
		expect(convertFileSrcMock).toHaveBeenCalledWith('C:\\theme-assets\\wall.png');
	});

	it('persists custom themes, image bytes, and paths through the host contract', async () => {
		const saved = theme({ id: 'custom-saved' });
		invokeMock.mockImplementation(async (command: string) => {
			if (command === 'save_user_theme') return saved;
			if (command === 'get_theme_data') return { version: 1, themes: [saved] };
			if (command === 'save_theme_bg_image') return 'wall.png';
			if (command === 'save_theme_bg_image_from_path') return 'path.png';
			if (command === 'delete_user_theme') return undefined;
			throw new Error(`unexpected command ${command}`);
		});
		expect(await saveCustomTheme(saved)).toEqual(saved);
		expect(await saveThemeBgImage(new Uint8Array([1, 2]), 'png')).toBe('wall.png');
		expect(await saveThemeBgImageFromPath('C:\\wall.png')).toBe('path.png');
		await deleteCustomTheme(saved.id);
		expect(invokeMock).toHaveBeenCalledWith('delete_user_theme', { id: saved.id });
	});

	it('refreshes theme state and publishes a resolved background descriptor', async () => {
		const loaded = theme({ id: 'refresh', bgImage: 'bg.png', bgImageOpacity: 0.4 });
		invokeMock.mockImplementation(async (command: string) => {
			if (command === 'get_theme_data') return { version: 1, themes: [loaded] };
			if (command === 'get_theme_assets_dir') return 'C:\\assets';
			throw new Error(`unexpected command ${command}`);
		});
		await refreshThemes();
		await setActiveBgImage('refresh');
		expect(get(activeBgImage)).toEqual({ url: 'asset://C:\\theme-assets\\bg.png', opacity: 0.4 });
	});
});
