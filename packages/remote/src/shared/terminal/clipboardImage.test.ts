import { afterEach, describe, expect, it, vi } from 'vitest';

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke }));

import { acquireClipboardImagePath, imagePathFromClipboardEvent } from './clipboardImage';

const webRemote = import.meta.env.RIDGE_WEB_REMOTE === true;

afterEach(() => {
	vi.clearAllMocks();
	vi.unstubAllGlobals();
});

describe('clipboard image bridge', () => {
	it.skipIf(webRemote)('asks the host to save a desktop clipboard image', async () => {
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

	it.skipIf(!webRemote)('returns null when browser clipboard has no image item', async () => {
		vi.stubGlobal('navigator', { clipboard: { read: vi.fn(async () => [{ types: ['text/plain'] }]) } });
		const module = await import('./clipboardImage');

		expect(await module.acquireClipboardImagePath()).toBeNull();
		expect(invoke).not.toHaveBeenCalled();
	});

	it.skipIf(!webRemote)('returns null when browser clipboard access is unavailable', async () => {
		vi.stubGlobal('navigator', { clipboard: {} });
		const module = await import('./clipboardImage');

		expect(await module.acquireClipboardImagePath()).toBeNull();
	});

	it.skipIf(!webRemote)('returns null when browser clipboard read is rejected', async () => {
		vi.stubGlobal('navigator', { clipboard: { read: vi.fn(async () => { throw new Error('denied'); }) } });
		const module = await import('./clipboardImage');

		expect(await module.acquireClipboardImagePath()).toBeNull();
	});

	it.skipIf(!webRemote)('reads the first browser image and saves it through the host', async () => {
		const png = Uint8Array.from([0x89, 0x50, 0x4e, 0x47]);
		const blob = { arrayBuffer: vi.fn(async () => png.buffer) };
		vi.stubGlobal('navigator', {
			clipboard: {
				read: vi.fn(async () => [{ types: ['text/plain', 'image/png'], getType: vi.fn(async () => blob) }]),
			},
		});
		invoke.mockResolvedValueOnce('C:\\Temp\\remote-paste.png');
		const module = await import('./clipboardImage');

		expect(await module.acquireClipboardImagePath()).toBe('C:\\Temp\\remote-paste.png');
		expect(invoke).toHaveBeenCalledWith('save_clipboard_image_to_temp', { pngBase64: 'iVBORw==' });
	});
});
