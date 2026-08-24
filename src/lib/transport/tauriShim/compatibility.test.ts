import { afterEach, describe, expect, it, vi } from 'vitest';

import { bridge } from './bridge';
import { Channel, convertFileSrc, isTauri, transformCallback } from './core';
import { ask, confirm, message, open, save } from './dialog';
import { emit, emitTo, listen, once } from './event';
import { getCurrentWindow } from './window';

afterEach(() => {
	bridge.detach();
	vi.restoreAllMocks();
	vi.unstubAllGlobals();
});

function transportRig() {
	let controlHandler: ((frame: unknown) => void) | undefined;
	let paneHandler: ((paneId: string, bytes: Uint8Array) => void) | undefined;
	const sent: unknown[] = [];
	const transport = {
		sendControl: (frame: unknown) => sent.push(frame),
		onControl: (handler: (frame: unknown) => void) => {
			controlHandler = handler;
			return () => { controlHandler = undefined; };
		},
		sendPaneBytes: vi.fn(),
		onPaneBytes: (handler: (paneId: string, bytes: Uint8Array) => void) => {
			paneHandler = handler;
			return () => { paneHandler = undefined; };
		},
		connect: vi.fn(),
		close: vi.fn(),
		state: () => 'connected' as const,
		onStateChange: () => () => {},
		authState: () => 'authorized' as const,
		onAuthChange: () => () => {},
	};
	return { transport, sent, dispatchControl: (frame: unknown) => controlHandler?.(frame), dispatchPane: (id: string, bytes: Uint8Array) => paneHandler?.(id, bytes) };
}

describe('browser Tauri compatibility shims', () => {
	it('routes events and raw PTY bytes through the bridge', async () => {
		const rig = transportRig();
		bridge.attach(rig.transport as never, { useGlobalWorkspace: false });
		const eventHandler = vi.fn();
		const paneHandler = vi.fn();
		const stopEvent = await listen('pane-title', eventHandler);
		const stopPane = await listen('pty-output-pane-1', paneHandler);

		rig.dispatchControl({ type: 'event', name: 'pane-title', payload: { title: 'shell' } });
		const raw = new TextEncoder().encode('ready');
		rig.dispatchPane('pane-1', raw);

		expect(eventHandler).toHaveBeenCalledWith({ event: 'pane-title', id: 0, payload: { title: 'shell' } });
		expect(paneHandler).toHaveBeenCalledWith({ event: 'pty-output-pane-1', id: 0, payload: { data: 'ready' } });
		expect(paneHandler.mock.calls[0][0].payload.bytes).toBe(raw);

		const binary = new Uint8Array([0xff, 0x00, 0xe2, 0x28, 0xa1]);
		rig.dispatchPane('pane-1', binary);
		expect(paneHandler.mock.calls[1][0].payload.bytes).toBe(binary);
		stopEvent();
		stopPane();
	});

	it('supports once, subscribe-pane, and compatibility no-ops', async () => {
		const rig = transportRig();
		bridge.attach(rig.transport as never, { useGlobalWorkspace: false });
		const handler = vi.fn();
		await once('host-event', handler);
		rig.dispatchControl({ type: 'event', name: 'host-event', payload: 1 });
		rig.dispatchControl({ type: 'event', name: 'host-event', payload: 2 });
		bridge.subscribePane('pane-1', 'workspace-1', true);

		expect(handler).toHaveBeenCalledTimes(1);
		expect(rig.sent).toContainEqual(expect.objectContaining({ method: 'subscribe-pane' }));
		expect(isTauri()).toBe(true);
		expect(new Channel<number>().onmessage).toBeTypeOf('function');
		expect(transformCallback()).toBe(0);
		await emit('ignored', {});
		await emitTo('window', 'ignored', {});
	});

	it('maps host paths and special invoke commands', async () => {
		vi.stubGlobal('localStorage', { getItem: vi.fn(() => 'token/1') });
		const pathUrl = convertFileSrc('C:\\work tree\\main.ts');
		expect(pathUrl).toContain('path=C%3A%5Cwork+tree%5Cmain.ts');
		expect(pathUrl).toContain('token=token%2F1');

		const rig = transportRig();
		bridge.attach(rig.transport as never, { useGlobalWorkspace: false });
		const result = await (await import('./core')).invoke('set_pane_delta_mode', { paneId: 'pane-1' });
		expect(result).toBeUndefined();
		await (await import('./core')).invoke('register_pane_delta_channel', { paneId: 'pane-1', workspaceId: 'ws', active: true });
		expect(rig.sent).toContainEqual(expect.objectContaining({ method: 'subscribe-pane' }));
	});

	it('provides prompt-backed dialog behavior and fullscreen window shims', async () => {
		const prompt = vi.fn((_: string, seed?: string) => seed ? ` ${seed} ` : ' C:\\tmp\\file ');
		const alert = vi.fn();
		const confirmFn = vi.fn(() => true);
		vi.stubGlobal('window', {
			prompt,
			alert,
			confirm: confirmFn,
			innerWidth: 800,
			innerHeight: 600,
			addEventListener: vi.fn(),
			removeEventListener: vi.fn(),
		});
		vi.stubGlobal('document', {
			fullscreenElement: null,
			documentElement: { requestFullscreen: vi.fn() },
			exitFullscreen: vi.fn(),
		});
		const invoke = vi.spyOn(bridge, 'invoke').mockResolvedValue('C:\\host\\project');

		expect(await open({ directory: true, multiple: true })).toEqual(['C:\\host\\project']);
		expect(invoke).toHaveBeenCalledWith('get_current_project', {});
		expect(await save()).toBe('C:\\tmp\\file');
		expect(await confirm('continue?')).toBe(true);
		expect(await ask('again?')).toBe(true);
		await message('hello');
		expect(alert).toHaveBeenCalledWith('hello');

		const current = getCurrentWindow();
		expect(await current.isMaximized()).toBe(false);
		await current.maximize();
		expect((document.documentElement.requestFullscreen as ReturnType<typeof vi.fn>)).toHaveBeenCalledOnce();
		const resizeHandler = vi.fn();
		const off = await current.onResized(resizeHandler);
		expect((window.addEventListener as ReturnType<typeof vi.fn>)).toHaveBeenCalledWith('resize', expect.any(Function));
		off();
		expect((window.removeEventListener as ReturnType<typeof vi.fn>)).toHaveBeenCalledWith('resize', expect.any(Function));
	});
});
