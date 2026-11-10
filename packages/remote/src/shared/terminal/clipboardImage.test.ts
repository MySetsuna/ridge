import { fileURLToPath } from 'node:url';
import { runInNewContext } from 'node:vm';
import { build } from 'vite';
import { beforeAll, describe, expect, it, vi } from 'vitest';

const compiled = new Map<boolean, string>();

beforeAll(async () => {
	// Vitest coerces custom import.meta.env values to strings. Compile the
	// production boolean defines with Vite before executing either branch.
	for (const webRemote of [false, true]) {
		const result = await build({
			configFile: false,
			logLevel: 'silent',
			define: { 'import.meta.env.RIDGE_WEB_REMOTE': JSON.stringify(webRemote) },
			build: {
				write: false,
				minify: false,
				lib: {
					entry: fileURLToPath(new URL('./clipboardImage.ts', import.meta.url)),
					name: 'ClipboardImage',
					formats: ['iife'],
				},
				rollupOptions: {
					external: ['@tauri-apps/api/core'],
					output: { globals: { '@tauri-apps/api/core': 'clipboardHost' } },
				},
			},
		});
		const output = Array.isArray(result) ? result[0] : result;
		if (!output || !('output' in output)) throw new Error('Expected one clipboard build');
		const entry = output.output.find(chunk => chunk.type === 'chunk');
		if (!entry) throw new Error('Clipboard build has no entry');
		compiled.set(webRemote, entry.code);
	}
});

function clipboard(webRemote: boolean, browser: object = {}) {
	const invoke = vi.fn();
	const api = runInNewContext(`${compiled.get(webRemote)}\nClipboardImage`, {
		clipboardHost: { invoke }, navigator: browser, Uint8Array, btoa,
	}) as typeof import('./clipboardImage');
	return { api, invoke };
}

it('reads the host clipboard in the native build', async () => {
	const { api, invoke } = clipboard(false);
	invoke.mockResolvedValueOnce('C:\\Temp\\clipboard.png');
	expect(await api.acquireClipboardImagePath()).toBe('C:\\Temp\\clipboard.png');
	expect(invoke).toHaveBeenCalledExactlyOnceWith('read_clipboard_image_to_temp');
});

describe.each([['native', false], ['web', true]] as const)('%s paste events', (_name, webRemote) => {
	it('returns null for events without an image', async () => {
		const { api, invoke } = clipboard(webRemote);
		expect(await api.imagePathFromClipboardEvent({ clipboardData: null } as ClipboardEvent)).toBeNull();
		expect(await api.imagePathFromClipboardEvent({ clipboardData: { items: [] } } as unknown as ClipboardEvent)).toBeNull();
		expect(invoke).not.toHaveBeenCalled();
	});

	it('encodes the first image file and saves it through the host', async () => {
		const { api, invoke } = clipboard(webRemote);
		const file = { arrayBuffer: vi.fn(async () => Uint8Array.from([0x89, 0x50, 0x4e, 0x47]).buffer) };
		const item = { kind: 'file', type: 'image/png', getAsFile: () => file };
		invoke.mockResolvedValueOnce('C:\\Temp\\paste.png');
		expect(await api.imagePathFromClipboardEvent({ clipboardData: { items: [item] } } as unknown as ClipboardEvent)).toBe('C:\\Temp\\paste.png');
		expect(invoke).toHaveBeenCalledExactlyOnceWith('save_clipboard_image_to_temp', { pngBase64: 'iVBORw==' });
	});

	it('captures the event file before asynchronous reading yields', async () => {
		const { api, invoke } = clipboard(webRemote);
		let finishRead!: (value: ArrayBuffer) => void;
		const file = { arrayBuffer: () => new Promise<ArrayBuffer>(resolve => { finishRead = resolve; }) };
		const item = { kind: 'file', type: 'image/png', getAsFile: vi.fn(() => file) };
		const pending = api.imagePathFromClipboardEvent({ clipboardData: { items: [item] } } as unknown as ClipboardEvent);
		expect(item.getAsFile).toHaveBeenCalledOnce();
		expect(invoke).not.toHaveBeenCalled();
		item.getAsFile.mockImplementation(() => { throw new Error('event expired'); });
		invoke.mockResolvedValueOnce('C:\\Temp\\paste.png');
		finishRead(Uint8Array.from([0x89, 0x50, 0x4e, 0x47]).buffer);
		expect(await pending).toBe('C:\\Temp\\paste.png');
	});
});

describe('web build browser clipboard', () => {
	it('returns null when the clipboard has no image item', async () => {
		const read = vi.fn(async () => [{ types: ['text/plain'] }]);
		const { api, invoke } = clipboard(true, { clipboard: { read } });
		expect(await api.acquireClipboardImagePath()).toBeNull();
		expect(read).toHaveBeenCalledOnce();
		expect(invoke).not.toHaveBeenCalled();
	});

	it('returns null when the clipboard API is unavailable', async () => {
		const { api, invoke } = clipboard(true);
		expect(await api.acquireClipboardImagePath()).toBeNull();
		expect(invoke).not.toHaveBeenCalled();
	});

	it('returns null when clipboard permission is rejected', async () => {
		const { api, invoke } = clipboard(true, { clipboard: { read: vi.fn().mockRejectedValue(new Error('denied')) } });
		expect(await api.acquireClipboardImagePath()).toBeNull();
		expect(invoke).not.toHaveBeenCalled();
	});

	it('reads the first browser image and saves its exact bytes on the host', async () => {
		const blob = { arrayBuffer: vi.fn(async () => Uint8Array.from([0x89, 0x50, 0x4e, 0x47]).buffer) };
		const getType = vi.fn(async () => blob);
		const read = vi.fn(async () => [{ types: ['text/plain', 'image/png'], getType }]);
		const { api, invoke } = clipboard(true, { clipboard: { read } });
		invoke.mockResolvedValueOnce('C:\\Temp\\remote paste.png');
		expect(await api.acquireClipboardImagePath()).toBe('C:\\Temp\\remote paste.png');
		expect(read).toHaveBeenCalledOnce();
		expect(getType).toHaveBeenCalledExactlyOnceWith('image/png');
		expect(invoke).toHaveBeenCalledExactlyOnceWith('save_clipboard_image_to_temp', { pngBase64: 'iVBORw==' });
	});

	it('propagates an image read failure without reading the host clipboard', async () => {
		const failure = new Error('image read failed');
		const item = { types: ['image/png'], getType: vi.fn().mockRejectedValue(failure) };
		const { api, invoke } = clipboard(true, { clipboard: { read: vi.fn(async () => [item]) } });
		await expect(api.acquireClipboardImagePath()).rejects.toBe(failure);
		expect(invoke).not.toHaveBeenCalled();
	});

	it('propagates a host save failure with a single write attempt', async () => {
		const failure = new Error('save failed');
		const item = { types: ['image/png'], getType: async () => ({ arrayBuffer: async () => Uint8Array.from([0x89]).buffer }) };
		const { api, invoke } = clipboard(true, { clipboard: { read: vi.fn(async () => [item]) } });
		invoke.mockRejectedValueOnce(failure);
		await expect(api.acquireClipboardImagePath()).rejects.toBe(failure);
		expect(invoke).toHaveBeenCalledExactlyOnceWith('save_clipboard_image_to_temp', { pngBase64: 'iQ==' });
	});
});
