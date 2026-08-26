import { describe, expect, it, vi } from 'vitest';

import { loadHostTerminalFonts, parseCssFontFamilies } from './fontDataService';

describe('terminal font data service', () => {
	it('parses quoted CSS family names and removes duplicates', () => {
		expect(parseCssFontFamilies("'Font, One', \"Font Two\", monospace, FONT TWO"))
			.toEqual(['Font, One', 'Font Two', 'monospace']);
	});

	it('installs Host font payloads returned through invoke RPC', async () => {
		const installed: number[][] = [];
		const hash = 'a'.repeat(64);
		const invokeCommand = vi.fn(async (command: string) => command === 'load_terminal_font_faces'
			? { stackHash: '1'.repeat(64), faces: [{ family: 'Consolas', contentHash: hash, byteLen: 4 }] }
			: {
				contentHash: hash,
				offset: 0,
				byteLen: 4,
				dataBase64: btoa(String.fromCharCode(0, 1, 2, 255)),
				eof: true,
			});

		await expect(loadHostTerminalFonts(
			"'Consolas',monospace",
			(data) => { installed.push([...data]); },
			invokeCommand as never,
		)).resolves.toBe(1);
		expect(invokeCommand).toHaveBeenCalledWith('load_terminal_font_faces', {
			families: ['Consolas', 'monospace'],
			knownHashes: [],
		});
		expect(invokeCommand).toHaveBeenCalledWith('read_terminal_font_face_chunk', {
			contentHash: hash,
			offset: 0,
			length: 4,
		});
		expect(installed).toEqual([[0, 1, 2, 255]]);
	});

	it('rejects a Host face whose declared length does not match its payload', async () => {
		const invokeCommand = vi.fn(async () => ({
			stackHash: '2'.repeat(64),
			faces: [{
				family: 'Consolas',
				contentHash: 'b'.repeat(64),
				byteLen: 9,
				dataBase64: btoa('font'),
			}],
		}));
		await expect(loadHostTerminalFonts('Consolas', () => undefined, invokeCommand as never))
			.rejects.toThrow('FONT_DATA_INVALID');
	});

	it('rejects the legacy array response with a stable protocol error', async () => {
		const invokeCommand = vi.fn(async () => []);
		await expect(loadHostTerminalFonts('Consolas', () => undefined, invokeCommand as never))
			.rejects.toThrow('FONT_DATA_INVALID: Host font manifest has an unsupported shape');
	});

	it('rejects aggregate Host font bytes before requesting any chunks', async () => {
		const invokeCommand = vi.fn(async () => ({
			stackHash: 'c'.repeat(64),
			faces: Array.from({ length: 4 }, (_, index) => ({
				family: `Face ${index}`,
				contentHash: index.toString(16).padStart(64, '0'),
				byteLen: 32 * 1024 * 1024,
			})),
		}));
		await expect(loadHostTerminalFonts('Consolas', () => undefined, invokeCommand as never))
			.rejects.toThrow('FONT_DATA_LIMIT: Host font manifest exceeds the 96 MiB limit');
		expect(invokeCommand).toHaveBeenCalledTimes(1);
	});
});
