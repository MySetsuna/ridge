import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const wasm = vi.hoisted(() => {
	class FakeKernel {
		static instances: FakeKernel[] = [];
		private readonly rowCount = 24;
		private readonly colCount = 80;
		private readonly scrollback = 10;
		mouseMode = 0;
		focusReporting = false;
		constructor(..._args: unknown[]) {
			FakeKernel.instances.push(this);
		}
		rows = vi.fn(() => this.rowCount);
		cols = vi.fn(() => this.colCount);
		scrollbackLen = vi.fn(() => this.scrollback);
		scrollOffset = vi.fn(() => 0);
		feed = vi.fn();
		applyDeltaFrame = vi.fn();
		takePendingResponse = vi.fn(() => new Uint8Array());
		takePendingEvents = vi.fn(() => []);
		isFocusReporting = vi.fn(() => this.focusReporting);
		mouseReportingModes = vi.fn(() => this.mouseMode);
		encodeMouse = vi.fn(() => new Uint8Array([0x6d]));
		hyperlinkAt = vi.fn(() => null);
		setSelectionAbs = vi.fn();
		selectWordAt = vi.fn();
		selectLineAt = vi.fn();
		free = vi.fn();
		isAltScreen = vi.fn(() => false);
		isInlineTuiMode = vi.fn(() => false);
		scrollUp = vi.fn();
		scrollDown = vi.fn();
		isAppCursorKeys = vi.fn(() => false);
		isCursorVisible = vi.fn(() => true);
		dumpVisibleText = vi.fn(() => ['visible line']);
		cursorRow = vi.fn(() => 2);
		cursorCol = vi.fn(() => 3);
		lastAbsCsiPosition = vi.fn(() => null);
		getSelectionText = vi.fn(() => 'selected');
		hasSelection = vi.fn(() => true);
		e2eEncodeCursorDeltaFrame = vi.fn((seq: number, row: number, col: number) =>
			new Uint8Array([seq, row, col]));
		shouldAllowShellHistory = vi.fn(() => true);
		isMouseReporting = vi.fn(() => this.mouseMode !== 0);
		backendName = vi.fn(() => 'WebGPU');
		setPresentFast = vi.fn();
	}

	class FakeRenderHandle {
		static newWithWebgpuFirst = vi.fn(async () => new FakeRenderHandle());
		configure = vi.fn(() => [10, 20]);
		applyDefaultTheme = vi.fn();
		applyTheme = vi.fn();
		invalidateAll = vi.fn();
		repaintAll = vi.fn();
		free = vi.fn();
		setFocused = vi.fn();
		setPadding = vi.fn();
		resize = vi.fn();
		render = vi.fn();
		currentThemeProbe = vi.fn(() => new Uint8Array([
			1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255,
		]));
		presentedCursorRow = vi.fn(() => 2);
		presentedCursorCol = vi.fn(() => 3);
		backendName = vi.fn(() => 'WebGPU');
	}

	const host = {
		clone: vi.fn(),
		resize: vi.fn(),
		invalidate: vi.fn(),
		setWallpaper: vi.fn(),
		clearWallpaper: vi.fn(),
	};
	host.clone.mockReturnValue(host);

	return {
		FakeKernel,
		FakeRenderHandle,
		init: vi.fn(async () => undefined),
		atlasOverwriteAfterCiteCount: vi.fn(() => 7),
		setPresentFast: vi.fn(),
		SurfaceHostHandle: { init: vi.fn(async () => host) },
		host,
	};
});

const tauri = vi.hoisted(() => ({
	invoke: vi.fn(async () => undefined),
}));

vi.mock('@tauri-apps/api/core', () => tauri);

vi.mock('@ridge/term-wasm', () => ({
	default: wasm.init,
	TerminalKernel: wasm.FakeKernel,
	RenderHandle: wasm.FakeRenderHandle,
	SurfaceHostHandle: wasm.SurfaceHostHandle,
	atlasOverwriteAfterCiteCount: wasm.atlasOverwriteAfterCiteCount,
	setPresentFast: wasm.setPresentFast,
}));

import { TerminalManager } from './manager';

class FakeElement {
	style = {} as Record<string, string>;
	dataset = {} as Record<string, string>;
	children: FakeElement[] = [];
	isConnected = true;
	parentElement: FakeElement | null = null;
	addEventListener = vi.fn((type: string, listener: (event: unknown) => void) => {
		(this.listeners as Map<string, (event: unknown) => void>).set(type, listener);
	});
	removeEventListener = vi.fn();
	appendChild = vi.fn((child: FakeElement) => {
		child.parentElement = this;
		this.children.push(child);
		return child;
	});
	remove = vi.fn(() => { this.isConnected = false; });
	setAttribute = vi.fn();
	getBoundingClientRect = vi.fn(() => ({
		x: 0, y: 0, left: 0, top: 0, right: 800, bottom: 400, width: 800, height: 400,
	}));
	closest = vi.fn(() => null);
	setPointerCapture = vi.fn();
	releasePointerCapture = vi.fn();
	private listeners = new Map<string, (event: unknown) => void>();

	emit(type: string, event: unknown = {}): void {
		this.listeners.get(type)?.(event);
	}
}

class FakeCanvas extends FakeElement {
	width = 800;
	height = 400;
	getContext = vi.fn(() => null);
}

function makeContainer(): FakeElement {
	return new FakeElement();
}

beforeEach(() => {
	vi.useFakeTimers();
	wasm.FakeKernel.instances.length = 0;
	vi.stubGlobal('HTMLCanvasElement', FakeCanvas);
	vi.stubGlobal('ResizeObserver', class {
		observe = vi.fn();
		disconnect = vi.fn();
	});
	vi.stubGlobal('requestAnimationFrame', vi.fn(() => 1));
	vi.stubGlobal('cancelAnimationFrame', vi.fn());
	vi.stubGlobal('localStorage', {
		getItem: vi.fn(() => null),
		setItem: vi.fn(),
		removeItem: vi.fn(),
	});
	vi.stubGlobal('navigator', {
		platform: '',
		clipboard: { writeText: vi.fn() },
	});
	vi.stubGlobal('window', {
		devicePixelRatio: 1,
		getComputedStyle: vi.fn(() => ({
			paddingLeft: '0px', paddingRight: '0px', paddingTop: '0px', paddingBottom: '0px',
		})),
		open: vi.fn(),
	});
	vi.stubGlobal('document', {
		hidden: false,
		createElement: vi.fn((tag: string) => tag === 'canvas' ? new FakeCanvas() : new FakeElement()),
		querySelector: vi.fn((selector: string) => selector === 'canvas[data-rg-host]' ? new FakeCanvas() : null),
		addEventListener: vi.fn(),
		removeEventListener: vi.fn(),
	});
	wasm.SurfaceHostHandle.init.mockResolvedValue(wasm.host);
	(TerminalManager as any)._instance = null;
	TerminalManager.setHostPorts(null);
});

afterEach(() => {
	vi.useRealTimers();
	vi.unstubAllGlobals();
	(TerminalManager as any)._instance = null;
});

describe('TerminalManager attach lifecycle', () => {
	it('initializes wasm once and exposes atlas/present-fast diagnostics', async () => {
		const manager = TerminalManager.instance({
			fontFamily: 'monospace', fontSizePx: 14, scrollbackLines: 200,
		});
		const storage = localStorage as unknown as { getItem: ReturnType<typeof vi.fn> };
		storage.getItem.mockImplementation((key: string) => key === 'RIDGE_PRESENT_FAST' ? '1' : null);

		await manager.ready();
		await manager.ready();

		expect(wasm.init).toHaveBeenCalledOnce();
		expect(wasm.setPresentFast).toHaveBeenCalledWith(true);
		expect((window as any).__ridgeAtlasRace()).toEqual({ overwriteAfterCite: 7 });
	});

	it('uses WebGPU-only handles and surfaces constructor failure', async () => {
		const manager = TerminalManager.instance({
			fontFamily: 'monospace', fontSizePx: 14, scrollbackLines: 200,
		});
		(manager as any).wasmReady = true;
		const ctor = wasm.FakeRenderHandle as any;
		const original = ctor.newWithWebgpuFirst;
		try {
			const preferred = new wasm.FakeRenderHandle();
			ctor.newWithWebgpuFirst = vi.fn(async () => preferred);
			await expect((manager as any)._makeHandle(new FakeCanvas(), wasm.host)).resolves.toBe(preferred);

			ctor.newWithWebgpuFirst = vi.fn(async () => { throw new Error('adapter unavailable'); });
			await expect((manager as any)._makeHandle(new FakeCanvas(), wasm.host))
				.rejects.toThrow('adapter unavailable');
			expect(ctor.newWithWebgpuFirst).toHaveBeenCalledOnce();
		} finally {
			ctor.newWithWebgpuFirst = original;
		}
	});

	it('keeps attach helpers fail-closed when host setup is unavailable', async () => {
		const manager = TerminalManager.instance({
			fontFamily: 'monospace', fontSizePx: 14, scrollbackLines: 200,
			theme: { background: '#010203' },
		});
		const internal = manager as any;
		internal.attachHostPromise = Promise.reject(new Error('host unavailable'));
		await expect(internal._awaitAttachHost()).rejects.toThrow('host unavailable');
		const host = { canvas: new FakeCanvas(), host: {} };
		internal.globalHost = host;
		const container = makeContainer();
		expect(internal._createAttachCanvas(container)).toEqual({ canvas: host.canvas, hostHandle: host.host });
		expect(container.style.background).toBe('transparent');
		const traced = localStorage as unknown as { getItem: ReturnType<typeof vi.fn> };
		traced.getItem.mockReturnValue('1');
		const handle = new wasm.FakeRenderHandle();
		internal._applyAttachTheme('pane-a', handle);
		expect(handle.applyDefaultTheme).toHaveBeenCalledOnce();
	});

	it('requires ready, attaches a configured pane, and fences duplicate ids', async () => {
		const manager = TerminalManager.instance({
			fontFamily: 'monospace',
			fontSizePx: 14,
			scrollbackLines: 200,
			paddingPx: 6,
			theme: { background: '#010203', foreground: '#fefefe' },
		});
		const container = makeContainer();

		await expect(manager.attach('pane-a', container as unknown as HTMLElement, 'workspace-a'))
			.rejects.toThrow('call ready() first');

		(manager as any).wasmReady = true;
		TerminalManager.setHostPorts({
			settings: { get: () => ({ terminalScrollbackLines: 321 }) },
		});
		await manager.attach('pane-a', container as unknown as HTMLElement, 'workspace-a');

		const entry = (manager as any).panes.get('pane-a');
		expect(entry).toBeDefined();
		expect(entry.canvas).toBeInstanceOf(FakeCanvas);
		expect(container.style.background).toBe('transparent');
		expect(container.style.padding).toBe('6px');
		expect(entry.resizeObserver.observe).toHaveBeenCalledWith(container);
		expect(wasm.FakeKernel.instances).toHaveLength(1);
		expect(wasm.FakeRenderHandle.prototype).toBeDefined();
		expect(entry.handle.configure).toHaveBeenCalledWith('monospace', 14, 1);
		expect(entry.handle.applyDefaultTheme).toHaveBeenCalledOnce();
		expect(entry.handle.applyTheme).toHaveBeenCalledWith({ background: '#010203', foreground: '#fefefe' });

		await expect(manager.attach('pane-a', makeContainer() as unknown as HTMLElement, 'workspace-a'))
			.rejects.toThrow('already attached');
		const handle = entry.handle;
		manager.detach('pane-a');
		expect(handle.free).toHaveBeenCalledOnce();
		expect(wasm.FakeKernel.instances[0]!.free).toHaveBeenCalledOnce();
		expect((manager as any).panes.has('pane-a')).toBe(false);
	});

	it('routes focus and pointer events through the attached kernel and cleans listeners', async () => {
		const manager = TerminalManager.instance({
			fontFamily: 'monospace', fontSizePx: 14, scrollbackLines: 200,
		});
		(manager as any).wasmReady = true;
		const container = makeContainer();
		await manager.attach('pane-a', container as unknown as HTMLElement, 'workspace-a');
		const entry = (manager as any).panes.get('pane-a');
		const kernel = wasm.FakeKernel.instances[0]!;
		const sent: Uint8Array[] = [];
		manager.onData('pane-a', (bytes) => sent.push(bytes));

		kernel.focusReporting = true;
		container.emit('focusin');
		container.emit('focusout');
		expect(sent.map((bytes) => new TextDecoder().decode(bytes))).toEqual(['\x1b[I', '\x1b[O']);

		const target = new FakeElement();
		kernel.mouseMode = 1;
		const pointer = {
			clientX: 35, clientY: 65, button: 0, buttons: 1, pointerId: 7,
			ctrlKey: false, metaKey: false, shiftKey: false, altKey: false,
			target,
		};
		container.emit('pointerdown', pointer);
		container.emit('pointerup', pointer);
		expect(kernel.encodeMouse).toHaveBeenCalled();
		expect(target.setPointerCapture).toHaveBeenCalledWith(7);
		expect(target.releasePointerCapture).toHaveBeenCalledWith(7);

		const modifiedPointer = { ...pointer, ctrlKey: true, shiftKey: true, altKey: true };
		container.emit('pointerdown', modifiedPointer);
		expect(kernel.encodeMouse).toHaveBeenLastCalledWith(
			expect.any(Number), expect.any(Number), 0, 0, true, true, true,
		);
		container.emit('pointerup', modifiedPointer);
		expect(kernel.encodeMouse).toHaveBeenLastCalledWith(
			expect.any(Number), expect.any(Number), 0, 1, true, true, true,
		);

		kernel.mouseMode = 0;
		container.emit('pointerdown', { ...pointer, detail: 2 });
		expect(kernel.selectWordAt).toHaveBeenCalled();
		container.emit('pointerdown', { ...pointer, detail: 3 });
		expect(kernel.selectLineAt).toHaveBeenCalled();

		manager.detach('pane-a');
		expect(entry.resizeObserver.disconnect).toHaveBeenCalledOnce();
		expect(container.removeEventListener).toHaveBeenCalledWith('focusin', entry.focusListener);
		expect(container.removeEventListener).toHaveBeenCalledWith('pointermove', entry.pointerMoveListener);
	});

	it('batches TUI motion, host drag auto-scroll, modifier hover, and cancellation', async () => {
		const manager = TerminalManager.instance({
			fontFamily: 'monospace', fontSizePx: 14, scrollbackLines: 200,
		});
		(manager as any).wasmReady = true;
		const container = makeContainer();
		await manager.attach('pane-a', container as unknown as HTMLElement, 'workspace-a');
		const kernel = wasm.FakeKernel.instances[0]!;
		const target = new FakeElement();
		const sent: Uint8Array[] = [];
		manager.onData('pane-a', (bytes) => sent.push(bytes));

		let frame: ((time: number) => void) | undefined;
		vi.stubGlobal('requestAnimationFrame', vi.fn((callback: (time: number) => void) => {
			frame = callback;
			return 1;
		}));
		const pointer = {
			clientX: 35, clientY: 200, button: 0, buttons: 1, pointerId: 7,
			ctrlKey: false, metaKey: false, shiftKey: false, altKey: false, target,
		};

		kernel.mouseMode = 2;
		container.emit('pointermove', { ...pointer, buttons: 1, clientX: 45, clientY: 85 });
		frame?.(0);
		expect(kernel.encodeMouse).toHaveBeenCalledWith(expect.any(Number), expect.any(Number), 0, 2, false, false, false);

		kernel.mouseMode = 4;
		container.emit('pointermove', { ...pointer, buttons: 0 });
		frame?.(1);
		container.emit('pointermove', { ...pointer, buttons: 0 });
		frame?.(2);
		expect(kernel.encodeMouse).toHaveBeenCalledWith(10, 3, 3, 2, false, false, false);
		expect(sent.length).toBeGreaterThanOrEqual(2);
		container.emit('pointerup', pointer);
		expect(kernel.encodeMouse).toHaveBeenCalledWith(10, 3, 0, 1, false, false, false);

		kernel.mouseMode = 0;
		container.emit('pointerdown', pointer);
		container.emit('pointermove', { ...pointer, clientY: 401 });
		vi.advanceTimersByTime(30);
		expect(kernel.scrollDown).toHaveBeenCalledWith(1);
		frame?.(2);
		container.emit('keydown', { key: 'Control', ctrlKey: true, metaKey: false });
		frame?.(3);
		container.emit('pointercancel', pointer);
		expect(target.releasePointerCapture).toHaveBeenCalledWith(7);
		container.emit('pointerleave');
		expect(container.style.cursor).toBe('');
	});

	it('exposes development diagnostics for PTY, kernel, theme, selection, and worker state', async () => {
		const manager = TerminalManager.instance({
			fontFamily: 'monospace', fontSizePx: 14, scrollbackLines: 200,
			theme: { background: '#010203', foreground: '#fefefe' },
		});
		(manager as any).wasmReady = true;
		const container = makeContainer();
		await manager.attach('pane-a', container as unknown as HTMLElement, 'workspace-a');
		const hooks = (window as unknown as { __windE2E?: Record<string, any> }).__windE2E;
		expect(hooks).toBeDefined();
		if (!hooks) return;

		manager.onData('pane-a', vi.fn());
		hooks.feedPty('pane-a', 'output');
		await hooks.writePty('pane-a', 'input');
		expect(tauri.invoke).toHaveBeenCalledWith('write_to_pty', { paneId: 'pane-a', data: 'input' });
		expect(hooks.visibleText('pane-a')).toEqual(['visible line']);
		expect(hooks.rows('pane-a')).toBe(24);
		expect(hooks.cols('pane-a')).toBe(80);
		expect(hooks.scrollbackLen('pane-a')).toBe(10);
		expect(hooks.themeSnapshot()).toEqual({ background: '#010203', foreground: '#fefefe' });
		expect(hooks.kernelCursor('pane-a')).toEqual({ row: 2, col: 3 });
		expect(hooks.presentedCursor('pane-a')).toEqual({ row: 2, col: 3 });
		expect(hooks.kernelThemeProbe('pane-a')).toEqual({
			bg: '#010203ff', fg: '#040506ff', cursor: '#070809ff', tuiBg: '#0a0b0cff',
		});
		hooks.setTheme({ background: '#111111', foreground: '#eeeeee' });
		expect(hooks.inputAnchorResolved('pane-a')).toMatchObject({ row: 2, col: 3, cellW: 10, cellH: 20 });
		expect(hooks.lastPreeditCall('pane-a')).toBeNull();
		hooks.kernelDecState('pane-a');

		hooks.setSelectionAbs('pane-a', 1, 2, 3, 4);
		expect(hooks.getSelectionText('pane-a')).toBe('selected');
		expect(hooks.hasSelection('pane-a')).toBe(true);
		hooks.applyDeltaFrameRaw('pane-a', new Uint8Array([1, 2]));
		expect(hooks.encodeCursorDeltaFrame('pane-a', 5, 6, 7)).toEqual(new Uint8Array([5, 6, 7]));

		hooks.installPtyWriteSpy('pane-a');
		manager.write('pane-a', 'echo');
		expect(hooks.ptyWriteLog('pane-a')).toHaveLength(1);
		const remountedHandler = vi.fn();
		manager.onData('pane-a', remountedHandler);
		hooks.installPtyWriteSpy('pane-a');
		manager.write('pane-a', 'after-remount');
		expect(hooks.ptyWriteLog('pane-a')).toHaveLength(2);
		expect(remountedHandler).toHaveBeenCalledOnce();
		hooks.clearPtyWriteLog('pane-a');
		expect(hooks.ptyWriteLog('pane-a')).toEqual([]);
		manager.detach('pane-a');
	});
});
