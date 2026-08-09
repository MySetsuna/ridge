import { afterEach, describe, expect, it, vi } from 'vitest';

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke }));

import { acquireClipboardImagePath, imagePathFromClipboardEvent } from './clipboardImage';

afterEach(() => {
	vi.clearAllMocks();
	vi.unstubAllGlobals();
});

describe('clipboard image bridge', () => {
	it('asks the host to save a desktop clipboard image', async () => {
		invoke.mockResolvedValueOnce('C:\\Temp\\clipboard.png');
		expect(await acquireClipboardImagePath()).toBe('C:\\Temp\\clipboard.png');
		expect(invoke).toHaveBeenCalledWith('read_clipboard_image_to_temp');
	});

	it('returns null for paste events without an image', async () => {
		expect(await imagePathFromClipboardEvent({ clipboardData: null } as ClipboardEvent)).toBeNull();
		expect(await imagePathFromClipboardEvent({ clipboardData: { items: [] } } as unknown as ClipboardEvent)).toBeNull();
	});

	it('encodes the first image file and saves it through the host', async () => {
		const file = {
			arrayBuffer: vi.fn(async () => Uint8Array.from([0x89, 0x50, 0x4e, 0x47]).buffer),
		};
		const item = { kind: 'file', type: 'image/png', getAsFile: () => file };
		invoke.mockResolvedValueOnce('C:\\Temp\\paste.png');

		expect(await imagePathFromClipboardEvent({ clipboardData: { items: [item] } } as unknown as ClipboardEvent)).toBe('C:\\Temp\\paste.png');
		expect(invoke).toHaveBeenCalledWith('save_clipboard_image_to_temp', { pngBase64: 'iVBORw==' });
	});
});
