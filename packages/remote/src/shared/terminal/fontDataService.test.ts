import { describe, expect, it, vi } from 'vitest';

import {
	loadBrowserTerminalFonts,
	loadNativeTerminalFonts,
	parseCssFontFamilies,
} from './fontDataService';

describe('terminal font data service', () => {
	it('parses quoted CSS family names and removes duplicates', () => {
		expect(parseCssFontFamilies("'Font, One', \"Font Two\", monospace, FONT TWO"))
			.toEqual(['Font, One', 'Font Two', 'monospace']);
	});

	it('installs native font payloads returned by the Tauri service', async () => {
		const installed: number[][] = [];
		const invokeCommand = vi.fn(async () => [
			{ family: 'Consolas', dataBase64: btoa(String.fromCharCode(0, 1, 2, 255)) },
		]);

		await expect(loadNativeTerminalFonts(
			"'Consolas',monospace",
			(data) => { installed.push([...data]); },
			invokeCommand as never,
		)).resolves.toBe(1);
		expect(invokeCommand).toHaveBeenCalledWith('load_terminal_font_faces', {
			families: ['Consolas', 'monospace'],
		});
		expect(installed).toEqual([[0, 1, 2, 255]]);
	});

	it('requires a user gesture before querying browser-local fonts', async () => {
		const queryLocalFonts = vi.fn(async () => []);
		await expect(loadBrowserTerminalFonts('Consolas', () => undefined, {
			hasTransientActivation: () => false,
			queryLocalFonts,
		})).rejects.toThrow('FONT_ACCESS_REQUIRED');
		expect(queryLocalFonts).not.toHaveBeenCalled();
	});

	it('filters browser-local faces to the selected family order', async () => {
		const installed: number[][] = [];
		const face = (family: string, postscriptName: string, bytes: number[]) => ({
			family,
			postscriptName,
			blob: async () => new Blob([new Uint8Array(bytes)]),
		});
		const source = {
			hasTransientActivation: () => true,
			queryLocalFonts: vi.fn(async () => [
				face('Unrelated', 'Unrelated-Regular', [9]),
				face('Segoe UI Emoji', 'SegoeUIEmoji', [3]),
				face('Consolas', 'Consolas', [1, 2]),
				face('Consolas', 'Consolas', [1, 2]),
			]),
		};

		await expect(loadBrowserTerminalFonts(
			"'Consolas','Segoe UI Emoji',monospace",
			(data) => { installed.push([...data]); },
			source,
		)).resolves.toBe(2);
		expect(installed).toEqual([[1, 2], [3]]);
	});
});
