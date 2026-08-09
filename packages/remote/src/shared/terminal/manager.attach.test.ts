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
		isAppCursorKeys = vi.fn(() => false);
		isCursorVisible = vi.fn(() => true);
		shouldAllowShellHistory = vi.fn(() => true);
		isMouseReporting = vi.fn(() => this.mouseMode !== 0);
		backendName = vi.fn(() => 'canvas2d');
	}

	class FakeRenderHandle {
		configure = vi.fn(() => [10, 20]);
		applyDefaultTheme = vi.fn();
		applyTheme = vi.fn();
		invalidateAll = vi.fn();
		free = vi.fn();
		setFocused = vi.fn();
		setPadding = vi.fn();
		resize = vi.fn();
	}

	return {
		FakeKernel,
		FakeRenderHandle,
		init: vi.fn(async () => undefined),
		SurfaceHostHandle: { init: vi.fn() },
	};
});

vi.mock('@ridge/term-wasm', () => ({
	default: wasm.init,
	TerminalKernel: wasm.FakeKernel,
	RenderHandle: wasm.FakeRenderHandle,
	SurfaceHostHandle: wasm.SurfaceHostHandle,
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
		querySelector: vi.fn(() => null),
		addEventListener: vi.fn(),
		removeEventListener: vi.fn(),
	});
	(TerminalManager as any)._instance = null;
	TerminalManager.setHostPorts(null);
});

afterEach(() => {
	vi.useRealTimers();
	vi.unstubAllGlobals();
	(TerminalManager as any)._instance = null;
});

describe('TerminalManager attach lifecycle', () => {
	it('requires ready, attaches a configured pane, and fences duplicate ids', async () => {
		const manager = TerminalManager.instance({
			fontFamily: 'monospace',
			fontSizePx: 14,
			scrollbackLines: 200,
			paddingPx: 6,
			preferWebgpu: false,
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
			fontFamily: 'monospace', fontSizePx: 14, scrollbackLines: 200, preferWebgpu: false,
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
});
