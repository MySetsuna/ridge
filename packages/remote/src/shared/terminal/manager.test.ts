import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { SurfaceHostHandle } from '@ridge/term-wasm';
import { MAX_PANE_FEED_FLUSH_BUDGET_MS, TerminalManager } from './manager';

const PANE = 'manager-test-pane';

function makeContainer() {
	return {
		style: {} as Record<string, string>,
		dataset: {} as Record<string, string>,
		addEventListener: vi.fn(),
		removeEventListener: vi.fn(),
		appendChild: vi.fn(),
		removeChild: vi.fn(),
		contains: vi.fn(() => true),
		closest: vi.fn(() => null),
		getBoundingClientRect: vi.fn(() => ({
			x: 10,
			y: 20,
			left: 10,
			top: 20,
			right: 810,
			bottom: 420,
			width: 800,
			height: 400,
		})),
	} as unknown as HTMLElement;
}

function makePane() {
	let rows = 24;
	let cols = 80;
	let offset = 0;
	let altScreen = false;
	let mouseModes = 0;
	const kernel = {
		rows: vi.fn(() => rows),
		cols: vi.fn(() => cols),
		feed: vi.fn(),
		applyDeltaFrame: vi.fn(),
		resize: vi.fn(),
		prependScrollback: vi.fn(),
		takePendingResponse: vi.fn(() => new Uint8Array()),
		takePendingEvents: vi.fn(() => []),
		isInlineTuiMode: vi.fn(() => false),
		isSyncOutput: vi.fn(() => false),
		isAltScreen: vi.fn(() => altScreen),
		backendName: vi.fn(() => 'WebGPU'),
		shouldAllowShellHistory: vi.fn(() => true),
		isMouseReporting: vi.fn(() => mouseModes !== 0),
		isAppCursorKeys: vi.fn(() => false),
		isCursorVisible: vi.fn(() => true),
		leaveAltScreen: vi.fn(),
		hyperlinkAt: vi.fn(() => null),
		lastAbsCsiPosition: vi.fn(() => null),
		cursorRow: vi.fn(() => 3),
		cursorCol: vi.fn(() => 4),
		scrollbackLen: vi.fn(() => 12),
		scrollOffset: vi.fn(() => offset),
		scrollToBottom: vi.fn(() => { offset = 0; }),
		scrollUp: vi.fn((n: number) => { offset += n; }),
		scrollDown: vi.fn((n: number) => { offset = Math.max(0, offset - n); }),
		encodeKey: vi.fn(() => new Uint8Array([0x41])),
		encodePaste: vi.fn(() => new Uint8Array([0x50])),
		encodeMouse: vi.fn(() => new Uint8Array([0x4d])),
		mouseReportingModes: vi.fn(() => mouseModes),
		appCursorKeys: vi.fn(() => false),
		getSelectionText: vi.fn(() => 'selected text'),
		clearSelection: vi.fn(),
		selectAll: vi.fn(),
		setSelectionAbs: vi.fn(),
		clearScrollback: vi.fn(),
		clearTerminalPreservingPrompt: vi.fn(),
		setPreedit: vi.fn(),
		searchSetQuery: vi.fn(() => 2),
		searchNext: vi.fn(() => 1),
		searchPrev: vi.fn(() => 0),
		searchClear: vi.fn(),
		searchActiveIndex: vi.fn(() => 0),
		searchMatchCount: vi.fn(() => 2),
		cellsAt: vi.fn((_row: number, _col: number, count: number) =>
			Array.from({ length: count }, (_, col) => ({
				col,
				ch: col === 0 ? 'A' : ' ',
				codepoint: col === 0 ? 0x41 : 0x20,
				width: 1,
				attrId: 0,
				dim: false,
				bold: false,
				italic: false,
				underline: false,
				inverse: false,
				hidden: false,
				fg: 'default',
				bg: 'default',
			}))),
	};
	const handle = {
		setFocused: vi.fn(),
		setPresentationCursorSuppressed: vi.fn(),
		setPreedit: vi.fn(),
		clearPreedit: vi.fn(),
		setHistoryOverlay: vi.fn(),
		clearHistoryOverlay: vi.fn(),
		backendName: vi.fn(() => 'WebGPU'),
		setFont: vi.fn(),
		setTheme: vi.fn(),
		applyDefaultTheme: vi.fn(),
		applyTheme: vi.fn(),
		configure: vi.fn(() => [10, 20]),
		forceFullRedraw: vi.fn(),
		invalidateAll: vi.fn(),
		repaintAll: vi.fn(),
		setPadding: vi.fn(),
		setViewportOffset: vi.fn(),
		resize: vi.fn(),
		render: vi.fn(),
		free: vi.fn(),
	};
	const container = makeContainer();
	const canvas = {
		width: 800,
		height: 400,
		style: {} as Record<string, string>,
		getBoundingClientRect: container.getBoundingClientRect,
	} as unknown as HTMLCanvasElement;
	const pane = {
		paneId: PANE,
		workspaceId: 'workspace-a',
		container,
		canvas,
		kernel,
		handle,
		cellW: 10,
		cellH: 20,
		lastConfiguredDpr: 1,
		resizeObserver: { disconnect: vi.fn(), observe: vi.fn() },
		lastReportedRows: -1,
		lastReportedCols: -1,
		pendingFitTimer: null,
		initialFitTimer: null,
		initialFitAttempt: 0,
		syncStart: null,
		syncTimeoutRendered: false,
		deltaQueue: [],
		deltaQueueHead: 0,
		deltaQueuedBytes: 0,
		tuiCursorSuppressUntil: 0,
		tuiCursorSuppressed: false,
		focusListener: vi.fn(),
		blurListener: vi.fn(),
		selecting: false,
		selectionStartAbs: null,
		selectionEndAbs: null,
		lastMouseSent: null,
		pendingMouseMove: null,
		mouseMoveRaf: null,
		autoScrollTimer: null,
		autoScrollDirection: null,
		pointerDownListener: vi.fn(),
		pointerMoveListener: vi.fn(),
		pointerUpListener: vi.fn(),
		pointerCancelListener: vi.fn(),
		pointerLeaveListener: vi.fn(),
		modifierKeyListener: vi.fn(),
		lastPointerPoint: null,
		parked: false,
		rendererRetained: false,
		parkReason: null,
		lastForegroundAt: 0,
		imeAnchor: null,
		imeAnchorRaf: null,
		feedBuffer: null,
		feedBufferChunks: [],
		feedBufferBytes: 0,
		feedFlushTimer: null,
		viewport: undefined,
		geometry: undefined,
		geometryVisualOffsetY: 0,
		visualOffsetY: 0,
		lastViewportKernelRows: -1,
		lastViewportKernelCols: -1,
	linkSpans: { markDirty: vi.fn(), clear: vi.fn(), hitTest: vi.fn(() => null) },
		linkUnderlineEls: [],
		linkUnderlineRegions: [],
		linkHintEl: null,
		linkHintRegion: null,
		lastScrollOffset: -1,
		lastScrollTotal: -1,
		scrollStateHandler: null,
		feedDeferred: null,
		feedDeferredChunks: [],
		feedDeferredBytes: 0,
		feedDroppedBytes: 0,
		feedDropCount: 0,
		feedNeedsResync: false,
		inputStartRow: null,
		inputStartCol: null,
	} as any;
	return {
		pane,
		kernel,
		handle,
		setRows: (value: number) => { rows = value; },
		setCols: (value: number) => { cols = value; },
		setOffset: (value: number) => { offset = value; },
		setAltScreen: (value: boolean) => { altScreen = value; },
		setMouseModes: (value: number) => { mouseModes = value; },
	};
}

function makeManager() {
	(TerminalManager as any)._instance = null;
	TerminalManager.setHostPorts(null);
	const manager = TerminalManager.instance({
		fontFamily: 'monospace',
		fontSizePx: 14,
		scrollbackLines: 200,
	});
	const internal = manager as any;
	internal.wasmReady = true;
	internal.loadedFontStacks.add('new-font');
	const fixture = makePane();
	internal.panes.set(PANE, fixture.pane);
	return { manager, fixture, internal };
}

beforeEach(() => {
	vi.stubGlobal('requestAnimationFrame', (cb: FrameRequestCallback) => {
		void cb;
		return 1;
	});
	vi.stubGlobal('cancelAnimationFrame', vi.fn());
	vi.stubGlobal('localStorage', {
		getItem: vi.fn(() => null),
		setItem: vi.fn(),
		removeItem: vi.fn(),
	});
	vi.stubGlobal('window', {
		devicePixelRatio: 1,
		open: vi.fn(),
		getComputedStyle: vi.fn(() => ({
			paddingLeft: '0px', paddingRight: '0px', paddingTop: '0px', paddingBottom: '0px',
		})),
	});
	if (globalThis.navigator && !('clipboard' in globalThis.navigator)) {
		Object.defineProperty(globalThis.navigator, 'clipboard', {
			configurable: true,
			value: { writeText: vi.fn() },
		});
	}
});

afterEach(() => {
	vi.unstubAllGlobals();
	(TerminalManager as any)._instance = null;
});

describe('TerminalManager public kernel and delivery surfaces', () => {
	it('projects host links and shared surface state through safe static boundaries', () => {
		const { manager, fixture } = makeManager();
		const openTextLink = vi.fn();
		TerminalManager.setHostPorts({
			cwd: {
				current: () => '/repo',
				workspaceRoot: () => '/repo',
				all: () => ['/repo', '/repo/packages'],
			},
			openTextLink,
		});
		expect(TerminalManager.tryInstance()).toBe(manager);
		expect(TerminalManager.hostPorts()?.cwd?.all()).toEqual(['/repo', '/repo/packages']);
		expect(TerminalManager._currentPaneCwd(fixture.pane)).toBe('/repo');
		expect(TerminalManager._knownCwds()).toEqual(['/repo', '/repo/packages']);

		expect(TerminalManager._executeOpenPlan({ type: 'noop', reason: 'unsupported' }, fixture.pane, 'x'))
			.toBe(false);
		expect(TerminalManager._executeOpenPlan({ type: 'open_file', path: 'src/main.ts', line: 4, col: 2 }, fixture.pane, 'x'))
			.toBe(true);
		expect(openTextLink).toHaveBeenCalledWith({
			type: 'path',
			path: 'src/main.ts',
			line: 4,
			col: 2,
			cwd: '/repo',
			workspaceRoot: '/repo',
			origin: { kind: 'local', workspaceId: 'workspace-a', paneId: 'manager-test-pane' },
		});
		expect(TerminalManager._executeOpenPlan({ type: 'reveal_in_tree', path: '/repo/src/main.ts' }, fixture.pane, 'x'))
			.toBe(true);
		expect(openTextLink).toHaveBeenLastCalledWith({
			type: 'path',
			path: '/repo/src/main.ts',
			directoryHint: true,
			cwd: '/repo',
			workspaceRoot: '/repo',
			origin: { kind: 'local', workspaceId: 'workspace-a', paneId: 'manager-test-pane' },
		});

		const host = {
			setWallpaper: vi.fn(),
			clearWallpaper: vi.fn(),
			invalidate: vi.fn(),
		};
		(manager as any).globalHost = { canvas: fixture.pane.canvas, host };
		manager.applyWallpaperGpu({ rgba: new Uint8Array([1, 2, 3, 4]), width: 2, height: 2, opacity: 0.5 });
		manager.applyWallpaperGpu(null);
		expect(host.setWallpaper).toHaveBeenCalledWith(new Uint8Array([1, 2, 3, 4]), 2, 2, 0.5);
		expect(host.clearWallpaper).toHaveBeenCalledOnce();
		TerminalManager.setHostPorts(null);
	});

	it('forwards feed, delta, write, paste, callbacks, and scroll state', () => {
		const { manager, fixture } = makeManager();
		const sent: Uint8Array[] = [];
		const events: unknown[] = [];
		manager.onData(PANE, (bytes) => sent.push(bytes));
		manager.onEvent(PANE, (event) => events.push(event));
		manager.onResize(PANE, vi.fn());
		manager.feed(PANE, 'hello');
		manager.applyDeltaFrame(PANE, new Uint8Array([1, 2]));
		manager.write(PANE, 'typed');
		manager.paste(PANE, 'paste');
		manager.sendData(PANE, new Uint8Array([9]));
		manager.prependScrollback(PANE, 'older');
		manager.selectAll(PANE);
		manager.clearSelection(PANE);
		manager.scrollUp(PANE, 2);
		manager.scrollDown(PANE, 1);
		manager.scrollToBottom(PANE);
		manager.clearScrollback(PANE);
		const unsubscribe = manager.onScrollState(PANE, (state) => events.push(state));
		(manager as any)._emitScrollStateChanges();
		unsubscribe();
		manager.resetInputModes(PANE);

		expect(fixture.kernel.feed).toHaveBeenCalled();
		expect(fixture.kernel.applyDeltaFrame).toHaveBeenCalledWith(new Uint8Array([1, 2]));
		expect(fixture.kernel.prependScrollback).toHaveBeenCalled();
		expect(fixture.kernel.selectAll).toHaveBeenCalledOnce();
		expect(sent.length).toBeGreaterThanOrEqual(3);
		expect(manager.getSelectionText(PANE)).toBe('selected text');
		expect(events).toEqual(expect.arrayContaining([{ offset: 0, total: 12 }]));
	});

	it('skips a duplicate dirty probe when output already marked the pane pending', () => {
		const { manager, fixture, internal } = makeManager();
		internal._activeWorkspaceId = fixture.pane.workspaceId;
		const isDirty = vi.fn(() => false);
		(fixture.handle as any).isDirty = isDirty;

		manager.feed(PANE, new Uint8Array([1, 2, 3]));
		expect(fixture.pane.renderPending).toBe(true);
		const state: any = {
			frameOrder: [fixture.pane],
			dateNow: Date.now(),
			perfNow: 10,
			anyRendered: false,
			minDeadlineMs: Infinity,
		};
		internal._renderFrameEntry(fixture.pane, state);

		expect(fixture.handle.render).toHaveBeenCalledOnce();
		expect(isDirty).not.toHaveBeenCalled();
		expect(fixture.pane.renderPending).toBe(false);

		internal._renderFrameEntry(fixture.pane, { ...state, anyRendered: false });
		expect(isDirty).toHaveBeenCalledOnce();
	});

	it('keeps remote feed callbacks on the bounded compositor queue', () => {
		const { manager, fixture, internal } = makeManager();
		const bytes = new Uint8Array([1, 2, 3]);

		expect(manager.enqueueFeed(PANE, bytes)).toBe(true);
		expect(fixture.kernel.feed).not.toHaveBeenCalled();
		internal._drainDeferredFeeds([fixture.pane]);

		expect(fixture.kernel.feed).toHaveBeenCalledWith(bytes);
		expect(fixture.pane.feedDeferredBytes).toBe(0);
	});

	it('drains native deltas on the frame hub, paints output, and freezes only a hinted TUI cursor', () => {
		const { manager, fixture, internal } = makeManager();
		fixture.kernel.applyDeltaFrame.mockReturnValueOnce(true);
		const bytes = new Uint8Array([1, 2, 3]);

		manager.enqueueDeltaFrame(PANE, bytes);
		expect(fixture.kernel.applyDeltaFrame).not.toHaveBeenCalled();
		expect(fixture.pane.deltaQueuedBytes).toBe(bytes.byteLength);

		internal._drainQueuedDeltaFrames([fixture.pane]);
		expect(fixture.kernel.applyDeltaFrame).toHaveBeenCalledWith(bytes);
		expect(fixture.pane.deltaQueuedBytes).toBe(0);
		expect(fixture.pane.tuiCursorSuppressUntil).toBeGreaterThan(0);
		expect(fixture.handle.setPresentationCursorSuppressed).toHaveBeenCalledWith(true);

		const held = {
			frameOrder: [fixture.pane], dateNow: Date.now(),
			perfNow: fixture.pane.tuiCursorSuppressUntil - 1,
			surfaceJustWiped: false, dirtyByPane: new Map(), activeWsId: null,
			activeHost: null, hostFrameOpen: false, anyRendered: false, minDeadlineMs: Infinity,
		};
		internal._renderFrameEntry(fixture.pane, held);
		expect(fixture.handle.render).toHaveBeenCalledOnce();

		held.perfNow = fixture.pane.tuiCursorSuppressUntil + 1;
		internal._renderFrameEntry(fixture.pane, held);
		expect(fixture.handle.setPresentationCursorSuppressed).toHaveBeenLastCalledWith(false);
		expect(fixture.handle.render).toHaveBeenCalledTimes(2);
	});

	it('keeps the cursor frozen after synchronized output until the quiet window expires', () => {
		const { manager, fixture, internal } = makeManager();
		fixture.kernel.applyDeltaFrame.mockReturnValueOnce(true);
		fixture.kernel.isSyncOutput.mockReturnValue(true);
		manager.enqueueDeltaFrame(PANE, new Uint8Array([7]));
		internal._drainQueuedDeltaFrames([fixture.pane]);
		fixture.handle.render.mockClear();

		const state = {
			frameOrder: [fixture.pane], dateNow: Date.now(), perfNow: 10,
			surfaceJustWiped: false, dirtyByPane: new Map(), activeWsId: null,
			activeHost: null, hostFrameOpen: false, anyRendered: false, minDeadlineMs: Infinity,
		};
		internal._renderFrameEntry(fixture.pane, state);
		expect(fixture.pane.syncStart).toBe(10);
		expect(fixture.handle.render).not.toHaveBeenCalled();

		fixture.kernel.isSyncOutput.mockReturnValue(false);
		state.perfNow = 11;
		internal._renderFrameEntry(fixture.pane, state);
		expect(fixture.handle.setPresentationCursorSuppressed).toHaveBeenLastCalledWith(true);
		expect(fixture.handle.render).toHaveBeenCalledOnce();

		state.perfNow = fixture.pane.tuiCursorSuppressUntil + 1;
		internal._renderFrameEntry(fixture.pane, state);
		expect(fixture.handle.setPresentationCursorSuppressed).toHaveBeenLastCalledWith(false);
	});

	it('drops queued native deltas after a decode error and reports it through the bridge callback', () => {
		const { manager, fixture, internal } = makeManager();
		const failure = vi.fn();
		fixture.kernel.applyDeltaFrame.mockImplementation(() => { throw new Error('bad postcard'); });
		manager.enqueueDeltaFrame(PANE, new Uint8Array([1]), failure);
		manager.enqueueDeltaFrame(PANE, new Uint8Array([2]), failure);

		internal._drainQueuedDeltaFrames([fixture.pane]);
		expect(failure).toHaveBeenCalledOnce();
		expect(fixture.pane.deltaQueue).toEqual([]);
		expect(fixture.pane.deltaQueuedBytes).toBe(0);
	});

	it('keeps compositor turns active while synchronized output still has queued parser work', () => {
		const { manager, fixture, internal } = makeManager();
		const raf = vi.fn(() => 7);
		vi.stubGlobal('requestAnimationFrame', raf);
		fixture.pane.deltaQueue.push({ bytes: new Uint8Array([1]) });
		fixture.kernel.isSyncOutput.mockReturnValue(true);
		const tick = vi.fn();
		const state = {
			frameOrder: [fixture.pane],
			anyRendered: false,
			minDeadlineMs: Infinity,
			dateNow: Date.now(),
			perfNow: 10,
			dirtyByPane: new Map([[PANE, true]]),
			activeHost: null,
			hostFrameOpen: false,
			surfaceJustWiped: false,
		};

		internal._renderFrameEntry(fixture.pane, state);
		expect(fixture.handle.render).not.toHaveBeenCalled();
		expect(state.minDeadlineMs).toBe(150);
		internal._scheduleNextFrame(state, tick);

		expect(raf).toHaveBeenCalledOnce();
		expect(raf).toHaveBeenCalledWith(tick);
		expect(internal.rafHandle).toBe(7);
		expect(internal.idleTimer).toBeNull();
	});

	it('handles key, mouse, wheel, search, selection, and overlay APIs', () => {
		const { manager, fixture } = makeManager();
		const sent: Uint8Array[] = [];
		manager.onData(PANE, (bytes) => sent.push(bytes));
		const key = {
			key: 'a', ctrlKey: false, metaKey: false, altKey: false, shiftKey: false,
		} as KeyboardEvent;
		expect(manager.handleKeyDown(PANE, key)).toBe(true);
		fixture.setMouseModes(1);
		expect(manager.handleWheel(PANE, {
			deltaY: -10, ctrlKey: false, shiftKey: false, altKey: false,
			clientX: 35, clientY: 65,
		} as WheelEvent)).toBe(true);
		fixture.setMouseModes(0);
		fixture.setAltScreen(true);
		expect(manager.wheelAltScroll(PANE, {
			deltaY: 240, ctrlKey: false, shiftKey: false, altKey: false,
			clientX: 35, clientY: 65,
		} as WheelEvent)).toBe(true);

		expect(manager.cellFromEvent(PANE, { clientX: 35, clientY: 65 })).toEqual({ row: 2, col: 2 });
		manager.updateSelection(PANE, { row: 8, col: 9 });
		manager.setPreedit(PANE, '拼', 3, 4);
		expect(manager.lastPreeditCall(PANE)).toEqual({ text: '拼', row: 3, col: 4 });
		manager.setHistoryOverlay(PANE, {
			items: ['one', 'two'],
			selectedIndex: 1,
			anchorRow: 2,
			anchorCol: 3,
			placeAbove: false,
			totalItems: 2,
			firstVisible: 0,
		});
		manager.clearHistoryOverlay(PANE);
		manager.clearPreedit(PANE);
		expect(manager.lastPreeditCall(PANE)).toBeNull();

		expect(manager.searchSetQuery(PANE, 'needle', true)).toBe(2);
		expect(manager.searchNext(PANE)).toBe(1);
		expect(manager.searchPrev(PANE)).toBe(0);
		expect(manager.searchInfo(PANE)).toEqual({ count: 2, activeIndex: 0 });
		manager.searchClear(PANE);
		manager.setFocused(PANE, true);
		manager.setFocused(PANE, false);
		manager.setVisualOffsetY(PANE, 12);
		manager.setLocalGridAuthority(PANE, false);
		manager.setPadding(PANE, 12);

		expect(sent.length).toBeGreaterThan(1);
		expect(fixture.kernel.encodeKey).toHaveBeenCalled();
		expect(fixture.kernel.encodeMouse).toHaveBeenCalled();
		expect(fixture.handle.setPreedit).toHaveBeenCalledWith('拼', 3, 4);
		expect(fixture.handle.clearHistoryOverlay).toHaveBeenCalledOnce();
	});

	it('reports bounded diagnostics and safe unknown-pane defaults', () => {
		const { manager, fixture } = makeManager();
		fixture.pane.feedDeferred = new Uint8Array([1, 2]);
		fixture.pane.feedDeferredBytes = 2;
		fixture.pane.feedDroppedBytes = 3;
		fixture.pane.feedDropCount = 1;
		fixture.pane.feedNeedsResync = true;
		expect(manager.feedStats(PANE)).toEqual({
			queuedBytes: 2,
			droppedBytes: 3,
			dropCount: 1,
			needsResync: true,
		});
		expect(manager.feedStats('missing')).toBeNull();
		expect(manager.rows('missing')).toBe(0);
		expect(manager.cols('missing')).toBe(0);
		expect(manager.getKernel('missing')).toBeNull();
		expect(manager.searchInfo('missing')).toEqual({ count: 0, activeIndex: -1 });
		expect(manager.scrollState('missing')).toEqual({ offset: 0, total: 0 });
		expect(manager.isSelecting('missing')).toBe(false);
		expect(manager.getMousePosition('missing')).toEqual({ row: 0, col: 0 });
		expect(manager.backendName(PANE)).toBe('WebGPU');
		expect(manager.backendName('missing')).toBeNull();
		expect(manager.isAltScreen(PANE)).toBe(false);
		expect(manager.debugDumpRows(PANE, 0, 0)[0]?.nonSpace[0]?.ch).toBe('A');
		expect(manager.debugGeometry()).toEqual([
			expect.objectContaining({
				scrollback: { offset: 0, total: 12 },
				cursor: { row: 3, col: 4 },
				visibleText: expect.any(Array),
			}),
		]);
	});

	it('keeps TUI gates, IME anchors, redraw invalidation, and theme state coherent', async () => {
		const { manager, fixture, internal } = makeManager();
		const anchors: Array<{ row: number; col: number } | null> = [];
		const unsubscribeIme = manager.onImeAnchor(PANE, (anchor) => anchors.push(anchor));
		expect(anchors).toEqual([null]);
		manager.markInputStart(PANE);
		expect(manager.readShellInputSnapshot(PANE)).toEqual(expect.objectContaining({ text: 'A', cursorCol: 0 }));
		expect(manager.inputAnchorCell(PANE)).toEqual({ row: 3, col: 4 });
		expect(manager.inputAnchorResolved(PANE)).toMatchObject({ row: 3, col: 4, x: 40, y: 60 });
		fixture.setAltScreen(true);
		fixture.kernel.lastAbsCsiPosition.mockReturnValue({ row: 5, col: 6, atMs: Date.now() });
		manager.beginImeComposition(PANE);
		fixture.kernel.cursorRow.mockReturnValue(7);
		fixture.kernel.cursorCol.mockReturnValue(8);
		expect(manager.inputAnchorCell(PANE)).toEqual({ row: 3, col: 4 });
		manager.endImeComposition(PANE);
		expect(manager.inputAnchorCell(PANE)).toEqual({ row: 7, col: 8 });
		expect(anchors.slice(-1)).toEqual([{ row: 7, col: 8 }]);
		expect(manager.pixelPositionFromCell(PANE, 2, 3)).toMatchObject({ x: 30, y: 40 });
		fixture.kernel.cursorRow.mockReturnValue(3);
		fixture.kernel.cursorCol.mockReturnValue(4);
		expect(manager.cursorPixelPosition(PANE)).toMatchObject({ x: 40, y: 60, fontSizePx: 14 });
		manager.clearInputStart(PANE);
		expect(manager.readShellInputSnapshot(PANE)).toBeNull();
		expect(anchors.slice(-1)).toEqual([null]);
		unsubscribeIme();

		expect(manager.shouldAllowShellHistory(PANE)).toBe(true);
		expect(manager.isMouseReporting(PANE)).toBe(false);
		expect(manager.isInlineTuiActive(PANE)).toBe(false);
		expect(manager.isAppCursorKeys(PANE)).toBe(false);
		expect(manager.isCursorVisible(PANE)).toBe(true);
		manager.leaveAltScreen(PANE);
		expect(fixture.kernel.leaveAltScreen).toHaveBeenCalledOnce();

		manager.forceFullRedraw(PANE);
		manager.forceFullRedrawFor([PANE, 'missing']);
		manager.invalidateWorkspace('workspace-a');
		manager.invalidateAllPanes();
		expect(fixture.handle.invalidateAll).toHaveBeenCalled();
		await manager.setFont('new-font', 16);
		expect(fixture.handle.configure).toHaveBeenCalledWith('new-font', 16, 1);
		manager.setTheme({ background: '#101010', foreground: '#f0f0f0' });
		expect(fixture.handle.applyDefaultTheme).toHaveBeenCalled();
		expect(fixture.handle.applyTheme).toHaveBeenCalledWith({ background: '#101010', foreground: '#f0f0f0' });
		manager.setSharedRemoteMode(true);
		manager.setSharedRemoteMode(false);
		manager.reclaimTerminalMemory({ forceHeapPressure: true });
		manager.restoreTerminalMemory();

		expect(manager.lastPreeditCall('missing')).toBeNull();
	});

	it('freezes a raw PTY cursor only when the parser reports repaint activity', () => {
		const { manager, fixture, internal } = makeManager();
		fixture.kernel.feed.mockReturnValueOnce(true);

		manager.feed(PANE, '\x1b[2K');
		expect(fixture.pane.tuiCursorSuppressUntil).toBeGreaterThan(0);
		expect(fixture.handle.setPresentationCursorSuppressed).toHaveBeenCalledWith(true);

		internal._releaseTuiCursorSuppression(fixture.pane);
		fixture.handle.setPresentationCursorSuppressed.mockClear();
		fixture.kernel.feed.mockReturnValueOnce(undefined);
		manager.feed(PANE, 'plain output');
		expect(fixture.handle.setPresentationCursorSuppressed).not.toHaveBeenCalledWith(true);
	});

	it('covers pointer/link routing, scroll subscriptions, padding, and lifecycle defaults', async () => {
		const { manager, fixture } = makeManager();
		const sent: Uint8Array[] = [];
		manager.onData(PANE, (bytes) => sent.push(bytes));
		fixture.setMouseModes(1);
		const target = { closest: vi.fn(() => null), setPointerCapture: vi.fn() };
		expect(manager.handlePointerDown(PANE, {
			clientX: 35, clientY: 65, button: 0, buttons: 1, pointerId: 2,
			ctrlKey: false, metaKey: false, shiftKey: false, altKey: false,
			target,
		} as unknown as PointerEvent)).toBe(true);
		expect(target.setPointerCapture).toHaveBeenCalledWith(2);
		const scrollbarTarget = { closest: vi.fn(() => '.rg-scrollbar-track') };
		expect(manager.handlePointerDown(PANE, {
			clientX: 35, clientY: 65, button: 0, buttons: 1, pointerId: 3,
			target: scrollbarTarget,
		} as unknown as PointerEvent)).toBe(false);

		const openTextLink = vi.fn();
		TerminalManager.setHostPorts({
			cwd: { current: () => '/repo', workspaceRoot: () => '/repo', all: () => ['/repo'] },
			openTextLink,
		});
		fixture.kernel.hyperlinkAt.mockReturnValue({ uri: 'https://example.com' });
		expect(manager.hasLinkAt(PANE, 1, 2)).toBe(true);
		expect(manager.openLinkAt(PANE, 1, 2)).toBe(true);
		expect(openTextLink).toHaveBeenLastCalledWith({
			type: 'url',
			href: 'https://example.com',
			cwd: '/repo',
			workspaceRoot: '/repo',
			origin: { kind: 'local', workspaceId: 'workspace-a', paneId: 'manager-test-pane' },
	});
		fixture.kernel.hyperlinkAt.mockReturnValue(null);
		fixture.pane.linkSpans.hitTest.mockReturnValue({ text: 'src/main.ts:4', kind: 'path' });
		expect(manager.hasLinkAt(PANE, 1, 2)).toBe(true);
		const ports = TerminalManager.hostPorts()!;
		expect(manager.openLinkAt(PANE, 1, 2)).toBe(true);
		fixture.pane.linkSpans.hitTest.mockReturnValue(null);
		expect(manager.hasLinkAt(PANE, 1, 2)).toBe(false);
		expect(ports.openTextLink).toHaveBeenLastCalledWith({
			type: 'path',
			path: '/repo/src/main.ts',
			line: 4,
			cwd: '/repo',
			workspaceRoot: '/repo',
			origin: { kind: 'local', workspaceId: 'workspace-a', paneId: 'manager-test-pane' },
		});

		const scrollEvents: Array<{ offset: number; total: number }> = [];
		const off = manager.onScrollState(PANE, (state) => scrollEvents.push(state));
		fixture.setOffset(5);
		(manager as any)._emitScrollStateChanges();
		expect(scrollEvents.at(-1)).toEqual({ offset: 5, total: 12 });
		off();
		manager.setPadding(PANE, 70);
		expect(fixture.pane.lastAppliedPaddingPx).toBe(64);
		await expect(manager.ready()).resolves.toBeUndefined();
		expect(manager.isParked('missing')).toBe(false);
		expect(manager.clearPendingFeed('missing')).toBe(0);
		expect(manager.prependScrollback('missing', 'x')).toBe(false);
		expect(sent.length).toBeGreaterThan(0);
	});

	it('keeps span and OSC hover affordances in sync with the pointer state', () => {
		const { manager, fixture } = makeManager();
		const entry = fixture.pane;
		const span = { row: 1, c0: 2, c1: 6, text: 'src/main.ts:4', kind: 'path' };
		entry.linkSpans.regionsForSpan = vi.fn(() => [span]);
		const showUnderline = vi.spyOn(manager as any, '_showLinkUnderlines').mockImplementation(() => undefined);
		const showHint = vi.spyOn(manager as any, '_showLinkHint').mockImplementation(() => undefined);
		vi.spyOn(manager as any, '_clearLinkUnderline').mockImplementation(() => undefined);

		(manager as any)._applySpanHover(entry, { row: 1, col: 3 }, span, true);
		expect(entry.container.dataset.linkUnderline).toBe('1:2:6');
		expect(showUnderline).toHaveBeenCalledWith(entry, [{ row: 1, c0: 2, c1: 6 }]);
		expect(showHint).toHaveBeenCalled();

		(manager as any)._applyOscHover(entry, { row: 2, col: 4 }, 'https://example.com', true);
		expect(entry.container.dataset.linkUnderline).toBe('2:4:5');
		(manager as any)._applyHoverDecision(entry, { row: 1, col: 3 }, null, span, {
			showUnderline: true, showHint: true, hintText: 'Ctrl+点击打开', cursor: 'pointer', spanText: span.text,
		});
		entry.container.style.cursor = 'pointer';
		entry.container.dataset.linkUnderline = '1:2:6';
		entry.linkUnderlineRegions = [span];
		(manager as any)._clearPointerHover(entry);
		expect(entry.container.style.cursor).toBe('');
	});

	it('plain link clicks select or forward while Ctrl+click opens once', () => {
		const { manager, fixture } = makeManager();
		const entry = fixture.pane;
		const open = vi.spyOn(manager, 'openLinkAt').mockReturnValue(true);
		entry.linkSpans.hitTest.mockReturnValue({ row: 1, c0: 2, c1: 6, text: 'src/main.ts', kind: 'path' });
		const target = { setPointerCapture: vi.fn() };
		const event = (ctrlKey: boolean) => ({
			clientX: 35, clientY: 65, button: 0, buttons: 1, pointerId: 2, detail: 1,
			ctrlKey, metaKey: false, shiftKey: false, altKey: false, target,
			preventDefault: vi.fn(),
		} as unknown as PointerEvent);

		fixture.setMouseModes(0);
		const plain = event(false);
		(manager as any)._handlePointerDown(PANE, plain);
		expect(open).not.toHaveBeenCalled();
		expect(entry.selecting).toBe(true);

		entry.selecting = false;
		const modified = event(true);
		(manager as any)._handlePointerDown(PANE, modified);
		expect(open).toHaveBeenCalledOnce();
		expect(modified.preventDefault).toHaveBeenCalledOnce();

		open.mockClear();
		fixture.setMouseModes(1);
		const tuiPlain = event(false);
		(manager as any)._handlePointerDown(PANE, tuiPlain);
		expect(open).not.toHaveBeenCalled();
		expect(fixture.kernel.encodeMouse).toHaveBeenCalled();
	});

	it('covers selection, input, scroll, and safe public projection edges', () => {
		const { manager, fixture } = makeManager();
		const sent: Uint8Array[] = [];
		manager.onData(PANE, (bytes) => sent.push(bytes));
		fixture.pane.selecting = true;
		fixture.pane.selectionStartAbs = { row: 2, col: 3 };
		manager.updateSelection(PANE, { row: 4, col: 5 });
		expect(fixture.kernel.setSelectionAbs).toHaveBeenCalledWith(2, 3, 4, 5);
		expect(manager.isSelecting(PANE)).toBe(true);
		expect(manager.getMousePosition(PANE)).toEqual({ row: 4, col: 5 });
		manager.sendData(PANE, new Uint8Array([7]));
		manager.write(PANE, new Uint8Array([8]));
		manager.paste(PANE, '粘贴');
		expect(sent).toHaveLength(3);
		expect(manager.getKernel(PANE)).toBe(fixture.kernel);
		manager.setVisualOffsetY(PANE, Number.NaN);
		expect(fixture.pane.visualOffsetY).toBe(0);

		fixture.setMouseModes(0);
		fixture.setAltScreen(false);
		expect(manager.handleWheel(PANE, { deltaY: 0 } as WheelEvent)).toBe(false);
		expect(manager.wheelAltScroll(PANE, { deltaY: 30, deltaMode: 1 } as WheelEvent)).toBe(false);
		fixture.setAltScreen(true);
		expect(manager.wheelAltScroll(PANE, { deltaY: 1, deltaMode: 1 } as WheelEvent)).toBe(true);
		manager.scrollUp(PANE, 2);
		manager.scrollDown(PANE, 1);
		fixture.setAltScreen(false);
		manager.clearTerminal(PANE);
		expect(fixture.kernel.clearTerminalPreservingPrompt).toHaveBeenCalledOnce();
		expect(sent.at(-1)).toEqual(new Uint8Array([0x0c]));
		manager.clearScrollback(PANE);
		expect(fixture.kernel.clearScrollback).toHaveBeenCalledOnce();
		expect(fixture.pane.linkSpans.clear).toHaveBeenCalledTimes(2);

		fixture.setAltScreen(true);
		manager.clearTerminalPreservingPrompt(PANE);
		expect(fixture.kernel.clearScrollback).toHaveBeenCalledTimes(2);
		expect(fixture.kernel.clearTerminalPreservingPrompt).toHaveBeenCalledOnce();

		fixture.kernel.getSelectionText.mockReturnValue('copy me');
		expect(manager.handleKeyDown(PANE, {
			key: 'c', ctrlKey: true, metaKey: false, altKey: false, shiftKey: false,
		} as KeyboardEvent)).toBe(true);
		expect(fixture.kernel.clearSelection).toHaveBeenCalled();
		fixture.kernel.getSelectionText.mockReturnValue('');
		manager.onResize(PANE, vi.fn());
		manager.setFocused(PANE, true);
		manager.setFocused(PANE, false);
		manager.leaveAltScreen(PANE);
		expect(fixture.kernel.leaveAltScreen).toHaveBeenCalled();

		fixture.handle.backendName.mockImplementationOnce(() => { throw new Error('renderer gone'); });
		expect(manager.backendName(PANE)).toBeNull();
		expect(manager.backendName('missing')).toBeNull();
		expect(manager.isCursorVisible('missing')).toBe(true);
		expect(manager.isMouseReporting('missing')).toBe(false);
		expect(manager.isInlineTuiActive('missing')).toBe(false);
		expect(manager.isAppCursorKeys('missing')).toBe(false);
		manager.clearSelection('missing');
		manager.clearScrollback('missing');
		manager.updateSelection('missing', { row: 1, col: 1 });
		expect(manager.getMousePosition('missing')).toEqual({ row: 0, col: 0 });
	});

	it('does not wipe the shared host when remounting a retained pane', () => {
		const { manager, fixture, internal } = makeManager();
		const host = { resize: vi.fn(), invalidate: vi.fn(), beginFrame: vi.fn(() => true), endFrame: vi.fn() };
		internal.globalHost = { canvas: fixture.pane.canvas, host };
		manager.park(PANE, 'component');
		expect(manager.isParked(PANE)).toBe(true);
		expect(fixture.handle.free).not.toHaveBeenCalled();
		expect(host.invalidate).not.toHaveBeenCalled();
	});

	it('parks and reclaims a pane without reviving renderer state', () => {
		const { manager, fixture } = makeManager();
		manager.park(PANE, 'memory');
		expect(manager.isParked(PANE)).toBe(true);
		expect(fixture.handle.free).toHaveBeenCalledOnce();
		expect(manager.feedStats(PANE)).toEqual({ queuedBytes: 0, droppedBytes: 0, dropCount: 0, needsResync: false });
		const reclaimed = manager.reclaimTerminalMemory({ forceHeapPressure: true });
		expect(reclaimed).toEqual(expect.objectContaining({ heapPressure: true, parkedPaneIds: [] }));
		manager.restoreTerminalMemory();
	});

	it('unparks a retained renderer and rejects invalid lifecycle transitions', async () => {
		const makeDomElement = () => ({
			style: {} as Record<string, string>,
			dataset: {} as Record<string, string>,
			setAttribute: vi.fn(),
			appendChild: vi.fn(),
			remove: vi.fn(),
			addEventListener: vi.fn(),
			removeEventListener: vi.fn(),
			getBoundingClientRect: vi.fn(() => ({ x: 0, y: 0, left: 0, top: 0, right: 800, bottom: 400, width: 800, height: 400 })),
			contains: vi.fn(() => true),
			closest: vi.fn(() => null),
		});
		vi.stubGlobal('document', {
			createElement: vi.fn(makeDomElement),
			documentElement: {},
			addEventListener: vi.fn(),
			removeEventListener: vi.fn(),
		});
		vi.stubGlobal('ResizeObserver', class {
			observe = vi.fn();
			disconnect = vi.fn();
		});
		const { manager, fixture, internal } = makeManager();
		(manager as any).globalHost = {
			canvas: fixture.pane.canvas,
			host: { resize: vi.fn(), invalidate: vi.fn(), beginFrame: vi.fn(() => true), endFrame: vi.fn() },
		};
		const container = makeDomElement();
		await expect(manager.unpark('missing', container as unknown as HTMLElement)).rejects.toThrow('not parked');
		manager.park(PANE);
		expect(manager.isParked(PANE)).toBe(true);
		await manager.unpark(PANE, container as unknown as HTMLElement);
		expect(manager.isParked(PANE)).toBe(false);
		expect(container.appendChild).not.toHaveBeenCalledWith(fixture.pane.canvas);
		expect(fixture.handle.configure).toHaveBeenCalled();
		await expect(manager.unpark(PANE, container as unknown as HTMLElement)).rejects.toThrow('already attached');
		manager.detach(PANE);
	});

	it('feeds inline-TUI fragments immediately and drains replies/events', () => {
		const { manager, fixture } = makeManager();
		const sent: Uint8Array[] = [];
		const events: unknown[] = [];
		manager.onData(PANE, (bytes) => sent.push(bytes));
		manager.onEvent(PANE, (event) => events.push(event));
		fixture.kernel.isInlineTuiMode.mockReturnValue(true);
		fixture.kernel.takePendingResponse.mockReturnValueOnce(new Uint8Array([0x52]));
		fixture.kernel.takePendingEvents.mockReturnValueOnce([{ type: 'Bell' }]);
		manager.feed(PANE, '\x1b[A');
		manager.feed(PANE, '\x1b[B');
		expect(fixture.kernel.feed).toHaveBeenNthCalledWith(1, new TextEncoder().encode('\x1b[A'));
		expect(fixture.kernel.feed).toHaveBeenNthCalledWith(2, new TextEncoder().encode('\x1b[B'));
		expect(sent).toEqual([new Uint8Array([0x52])]);
		expect(events).toEqual([{ type: 'Bell' }]);
		manager.feed(PANE, 'echo');
		expect(fixture.pane.feedBuffer).toBeNull();
		expect(fixture.kernel.feed).toHaveBeenLastCalledWith(new TextEncoder().encode('echo'));

		fixture.pane.feedDeferred = new Uint8Array([1, 2]);
		fixture.pane.feedDeferredBytes = 2;
		manager.feed(PANE, new Uint8Array([3, 4]));
		expect(fixture.pane.feedDeferredChunks).toHaveLength(1);
		manager.flushPaneFeed(PANE, MAX_PANE_FEED_FLUSH_BUDGET_MS);
		expect(fixture.pane.feedDeferredBytes).toBe(0);
		manager.clearPendingFeed(PANE);
	});

	it('does not delay shell escape markers when inline TUI is inactive', () => {
		vi.useFakeTimers();
		const { manager, fixture } = makeManager();
		fixture.kernel.isInlineTuiMode.mockReturnValue(false);
		const marker = new TextEncoder().encode('\x1b]133;C\x07');

		manager.feed(PANE, marker);

		expect(fixture.kernel.feed).toHaveBeenCalledWith(marker);
		expect(fixture.pane.feedBuffer).toBeNull();
		vi.useRealTimers();
	});

	it.each([
		['Codex', ['\x1b[4A\x1b[2K', 'Thinking', '\x1b[1B', 'Answer\x1b[?25h']],
		['Claude', ['\x1b[?25l', '\x1b[3A\x1b[2K', '\x1b[1B\x1b[2KDone', '\x1b[?25h']],
	])('feeds fragmented %s inline frames without a timer gap', (_name, fragments) => {
		const { manager, fixture } = makeManager();
		fixture.kernel.isInlineTuiMode.mockReturnValue(true);
		for (const fragment of fragments) {
			manager.feed(PANE, fragment);
		}
		expect(fixture.kernel.feed).toHaveBeenCalledTimes(fragments.length);
		for (const [index, fragment] of fragments.entries()) {
			expect(fixture.kernel.feed).toHaveBeenNthCalledWith(index + 1, new TextEncoder().encode(fragment));
		}
		expect(fixture.handle.render).not.toHaveBeenCalled();
	});

	it('flushes an inline buffer before a large escape packet and clears all pending bytes', () => {
		const { manager, fixture } = makeManager();
		fixture.kernel.isInlineTuiMode.mockReturnValue(true);
		manager.feed(PANE, '\x1b[1m');
		const older = new TextEncoder().encode('older');
		fixture.pane.feedBuffer = older;
		manager.feed(PANE, new Uint8Array(8192).fill(0x1b));
		expect(fixture.kernel.feed).toHaveBeenCalledWith(older);
		expect(fixture.kernel.feed.mock.calls.at(-1)?.[0]).toHaveLength(8192);

		fixture.pane.feedBuffer = new Uint8Array([1, 2]);
		fixture.pane.feedDeferred = new Uint8Array([3]);
		fixture.pane.feedDeferredChunks = [new Uint8Array([4, 5])];
		fixture.pane.feedDeferredBytes = 3;
		expect(manager.clearPendingFeed(PANE)).toBe(5);
		expect(manager.feedStats(PANE)).toEqual({ queuedBytes: 0, droppedBytes: 0, dropCount: 0, needsResync: false });

		const clock = vi.spyOn(performance, 'now');
		let calls = 0;
		clock.mockImplementation(() => (calls++ < 3 ? 0 : 2));
		fixture.pane.feedDeferred = new Uint8Array(32768).fill(1);
		fixture.pane.feedDeferredChunks = [new Uint8Array([9])];
		fixture.pane.feedDeferredBytes = 32769;
		manager.flushPaneFeed(PANE, 1);
		expect(fixture.pane.feedDeferred?.byteLength).toBe(16384);
		expect(fixture.pane.feedDeferredChunks).toEqual([new Uint8Array([9])]);
		clock.mockRestore();
	});

	it('preserves feed ordering across inline fragments and bounded deferred chunks', () => {
		const { manager, fixture } = makeManager();
		const sent: Uint8Array[] = [];
		manager.onData(PANE, (bytes) => sent.push(bytes));

		fixture.kernel.isInlineTuiMode.mockImplementationOnce(() => { throw new Error('old wasm'); });
		manager.feed(PANE, new Uint8Array([1]));
		fixture.kernel.isInlineTuiMode.mockReturnValue(true);
		manager.feed(PANE, new Uint8Array([0x1b, 0x41]));
		manager.feed(PANE, new Uint8Array([0x1b, 0x42]));
		expect(fixture.kernel.feed).toHaveBeenNthCalledWith(2, new Uint8Array([0x1b, 0x41]));
		expect(fixture.kernel.feed).toHaveBeenNthCalledWith(3, new Uint8Array([0x1b, 0x42]));
		manager.feed(PANE, 'echo');
		expect(fixture.kernel.feed).toHaveBeenLastCalledWith(new TextEncoder().encode('echo'));

		fixture.kernel.isInlineTuiMode.mockReturnValue(false);
		fixture.pane.feedBuffer = new Uint8Array([9]);
		fixture.pane.feedBufferBytes = 0;
		manager.feed(PANE, new Uint8Array([10]));
		expect(fixture.pane.feedBuffer).toBeNull();
		expect(fixture.kernel.feed.mock.calls.at(-2)?.[0]).toEqual(new Uint8Array([9]));
		expect(fixture.kernel.feed.mock.calls.at(-1)?.[0]).toEqual(new Uint8Array([10]));

		const clock = vi.spyOn(performance, 'now');
		clock.mockReturnValueOnce(0).mockReturnValue(10);
		manager.feed(PANE, new Uint8Array(20_000).fill(7));
		expect(fixture.pane.feedDeferredBytes).toBeGreaterThan(0);
		manager.feed(PANE, new Uint8Array([8]));
		expect(fixture.pane.feedDeferredBytes).toBeGreaterThan(0);
		manager.flushPaneFeed(PANE, MAX_PANE_FEED_FLUSH_BUDGET_MS);
		manager.clearPendingFeed(PANE);
		expect(manager.feedStats(PANE)).toEqual({ queuedBytes: 0, droppedBytes: 0, dropCount: 0, needsResync: false });
		clock.mockRestore();
		expect(sent).toEqual([]);
	});

	it('covers input ownership, wheel limits, and empty protocol responses', () => {
		const { manager, fixture, internal } = makeManager();
		const sent: Uint8Array[] = [];
		manager.onData(PANE, (bytes) => sent.push(bytes));
		vi.stubGlobal('navigator', { platform: 'MacIntel', clipboard: { writeText: vi.fn() } });

		fixture.kernel.getSelectionText.mockReturnValue('copy me');
		expect(manager.handleKeyDown(PANE, {
			key: 'c', ctrlKey: false, metaKey: true, altKey: false, shiftKey: false,
		} as KeyboardEvent)).toBe(true);
		expect((navigator.clipboard.writeText as ReturnType<typeof vi.fn>)).toHaveBeenCalledWith('copy me');
		fixture.kernel.getSelectionText.mockReturnValue('');
		fixture.kernel.encodeKey.mockReturnValueOnce(new Uint8Array());
		expect(manager.handleKeyDown(PANE, {
			key: 'x', ctrlKey: false, metaKey: false, altKey: false, shiftKey: false,
		} as KeyboardEvent)).toBe(false);

		fixture.setMouseModes(1);
		fixture.kernel.encodeMouse.mockReturnValueOnce(new Uint8Array());
		expect(manager.handleWheel(PANE, { deltaY: -10, clientX: 35, clientY: 65 } as WheelEvent)).toBe(false);
		fixture.setMouseModes(0);
		fixture.setAltScreen(true);
		fixture.kernel.encodeKey.mockReturnValue(new Uint8Array([0x41]));
		expect(manager.wheelAltScroll(PANE, { deltaY: 1000, deltaMode: 0 } as WheelEvent)).toBe(true);
		expect(sent.at(-1)).toHaveLength(5);
		fixture.kernel.encodeKey.mockReturnValueOnce(new Uint8Array());
		expect(manager.wheelAltScroll(PANE, { deltaY: 1, deltaMode: 1 } as WheelEvent)).toBe(false);

		fixture.kernel.takePendingResponse.mockReturnValueOnce(new Uint8Array([0x52]));
		manager.resetInputModes(PANE);
		expect(sent.at(-1)).toEqual(new Uint8Array([0x52]));
		fixture.kernel.rows.mockReturnValueOnce(0);
		expect(manager.cellFromEvent(PANE, { clientX: 1, clientY: 1 })).toBeNull();
		manager.updateSelection(PANE, { row: 1, col: 1 });
		manager.clearScrollback(PANE);
		manager.setFocused(PANE, true);
		manager.setFocused(PANE, false);
		internal.panes.delete(PANE);
		manager.onData(PANE, vi.fn());
		manager.onEvent(PANE, vi.fn());
		manager.onResize(PANE, vi.fn());
		manager.write(PANE, 'ignored');
	});

	it('covers shared-host resize boundaries and workspace invalidation', async () => {
		const { manager, fixture, internal } = makeManager();
		internal.panes.clear();
		const host = { resize: vi.fn(), invalidate: vi.fn(), beginFrame: vi.fn(() => true), endFrame: vi.fn() };
		internal.globalHost = { canvas: fixture.pane.canvas, host };

		manager.resizeHost({ wCss: 0, hCss: 20 });
		manager.resizeHost({ wCss: 100.8, hCss: 50.2 });
		expect(fixture.pane.canvas.width).toBe(100);
		expect(fixture.pane.canvas.height).toBe(50);
		expect(host.resize).toHaveBeenCalledWith(100, 50, 1);
		const calls = host.resize.mock.calls.length;
		manager.resizeHost({ wCss: 100.8, hCss: 50.2 });
		expect(host.resize).toHaveBeenCalledTimes(calls);

		manager.onActiveWorkspaceChanged('workspace-a');
		await Promise.resolve();
		expect(host.invalidate).toHaveBeenCalled();
		manager.detachHost();
		expect(internal.globalHost).toBeNull();
	});

	it('rolls back canvas dimensions when GPU surface resize rejects the viewport', () => {
		const { manager, fixture, internal } = makeManager();
		internal.panes.clear();
		fixture.pane.canvas.width = 80;
		fixture.pane.canvas.height = 24;
		fixture.pane.canvas.style.width = '80px';
		fixture.pane.canvas.style.height = '24px';
		const host = {
			resize: vi.fn(() => { throw new Error('WEBGPU_INIT_FAILED: WebGL2 surface exceeds adapter maximum'); }),
			invalidate: vi.fn(),
		};
		internal.globalHost = { canvas: fixture.pane.canvas, host };

		expect(() => manager.resizeHost({ wCss: 3100, hCss: 1348 }))
			.toThrow('WEBGPU_INIT_FAILED: WebGL2 surface exceeds adapter maximum');
		expect(fixture.pane.canvas).toMatchObject({ width: 80, height: 24 });
		expect(fixture.pane.canvas.style).toMatchObject({ width: '80px', height: '24px' });
		expect(host.invalidate).not.toHaveBeenCalled();
	});

	it('follows the live shell cursor but locks TUI composition to its start cell', () => {
		const { manager, fixture } = makeManager();
		fixture.kernel.cursorRow.mockReturnValue(3);
		fixture.kernel.cursorCol.mockReturnValue(4);
		manager.beginImeComposition(PANE);

		fixture.kernel.cursorRow.mockReturnValue(9);
		fixture.kernel.cursorCol.mockReturnValue(10);
		expect(manager.inputAnchorCell(PANE)).toEqual({ row: 9, col: 10 });

		fixture.setAltScreen(true);
		expect(manager.inputAnchorCell(PANE)).toEqual({ row: 3, col: 4 });
		manager.endImeComposition(PANE);
	});

	it('rejects invalid shell snapshots and resolves recent TUI cursor anchors', () => {
		const { manager, fixture } = makeManager();
		expect(manager.readShellInputSnapshot('missing')).toBeNull();
		expect(manager.readShellInputSnapshot(PANE)).toBeNull();
		fixture.pane.inputStartRow = 3;
		fixture.pane.inputStartCol = 4;
		fixture.kernel.cursorRow.mockReturnValueOnce(2);
		expect(manager.readShellInputSnapshot(PANE)).toBeNull();
		fixture.kernel.cursorRow.mockReturnValue(3);
		fixture.kernel.cursorCol.mockReturnValueOnce(3);
		expect(manager.readShellInputSnapshot(PANE)).toBeNull();
		fixture.kernel.cursorCol.mockReturnValue(7);
		fixture.kernel.cellsAt.mockReturnValue([
			{ ch: 'a', width: 1 }, { ch: 'b', width: 1 }, { ch: 'c', width: 1 },
		]);
		expect(manager.readShellInputSnapshot(PANE)).toEqual(expect.objectContaining({ text: expect.any(String) }));

		fixture.kernel.lastAbsCsiPosition.mockReturnValue({ row: 5, col: 6, atMs: Date.now() });
		fixture.setAltScreen(true);
		expect(manager.inputAnchorCell(PANE)).toEqual({ row: 5, col: 6 });
		expect(manager.inputAnchorPixelPosition(PANE)).toMatchObject({ x: 60, y: 100 });
	});

	it('covers shared host attach idempotence, wallpaper, and initialization failure', async () => {
		const { manager, fixture, internal } = makeManager();
		manager.detachHost();
		const ctor = SurfaceHostHandle as any;
		const originalInit = ctor.init;
		const host = {
			resize: vi.fn(),
			invalidate: vi.fn(),
			setWallpaper: vi.fn(),
			clearWallpaper: vi.fn(),
		};
		ctor.init = vi.fn(async () => host);
		try {
			await manager.attachHost(fixture.pane.canvas);
			await manager.attachHost(fixture.pane.canvas);
			await manager.attachHost({} as HTMLCanvasElement);
			expect(ctor.init).toHaveBeenCalledOnce();
			expect(internal.globalHost.host).toBe(host);
			manager.applyWallpaperGpu({
				rgba: new Uint8Array([1, 2, 3, 4]),
				width: 1,
				height: 1,
				opacity: 1,
			});
			manager.applyWallpaperGpu(null);
			expect(host.setWallpaper).toHaveBeenCalledOnce();
			expect(host.clearWallpaper).toHaveBeenCalledOnce();

			manager.detachHost();
			ctor.init = vi.fn(async () => { throw new Error('adapter unavailable'); });
			await expect(manager.attachHost(fixture.pane.canvas)).rejects.toThrow('adapter unavailable');
			expect(internal.globalHost).toBeNull();
		} finally {
			ctor.init = originalInit;
			manager.detachHost();
		}
	});

	it('defers inactive workspace theme work and skips parked panes', async () => {
		const { manager, fixture, internal } = makeManager();
		const other = makePane();
		other.pane.paneId = 'other-pane';
		other.pane.workspaceId = 'workspace-b';
		internal.panes.set(other.pane.paneId, other.pane);
		internal._activeWorkspaceId = 'workspace-b';
		manager.park(other.pane.paneId, 'component');

		const theme = { background: '#000', foreground: '#fff' };
		manager.setTheme(theme);
		await new Promise((resolve) => setTimeout(resolve, 5));
		expect(fixture.handle.applyTheme).toHaveBeenCalledWith(theme);
		expect(other.handle.applyTheme).not.toHaveBeenCalled();
	});

	it('orders live panes by focus and rotates non-focused work fairly', () => {
		const { manager, internal } = makeManager();
		const other = makePane();
		other.pane.paneId = 'other-pane';
		const third = makePane();
		third.pane.paneId = 'third-pane';
		internal.panes.set(other.pane.paneId, other.pane);
		internal.panes.set(third.pane.paneId, third.pane);

		const order = () => (manager as any)._renderOrder().map((entry: { paneId: string }) => entry.paneId);
		expect(order()).toEqual([PANE, 'other-pane', 'third-pane']);
		manager.setFocused('third-pane', true);
		expect(order()).toEqual(['third-pane', PANE, 'other-pane']);
		internal._rafRotationIndex = 1;
		expect(order()).toEqual(['third-pane', 'other-pane', PANE]);

		manager.park('other-pane', 'coverage');
		expect(order()).toEqual(['third-pane', PANE]);
		manager.setFocused('third-pane', false);
		expect(order()).toEqual(['third-pane', PANE]);
	});

	it('keeps focus and cursor ownership exclusive within each workspace', () => {
		const { manager, fixture, internal } = makeManager();
		const sibling = makePane();
		sibling.pane.paneId = 'sibling-pane';
		internal.panes.set(sibling.pane.paneId, sibling.pane);

		manager.setFocused(PANE, true);
		manager.setFocused(sibling.pane.paneId, true);
		expect(fixture.handle.setFocused).toHaveBeenLastCalledWith(false);
		expect(sibling.handle.setFocused).toHaveBeenLastCalledWith(true);
		manager.park(sibling.pane.paneId);
		expect(sibling.handle.setFocused).toHaveBeenLastCalledWith(false);

		const otherWorkspace = makePane();
		otherWorkspace.pane.paneId = 'other-workspace-pane';
		otherWorkspace.pane.workspaceId = 'workspace-b';
		internal.panes.set(otherWorkspace.pane.paneId, otherWorkspace.pane);
		manager.setFocused(otherWorkspace.pane.paneId, true);
		expect(sibling.handle.setFocused).toHaveBeenLastCalledWith(false);
		expect(otherWorkspace.handle.setFocused).toHaveBeenLastCalledWith(true);
	});

	it('debounces viewport fitting and skips parked or missing panes', async () => {
		vi.useFakeTimers();
		try {
			const { manager, fixture, internal } = makeManager();
			const fitPane = vi.spyOn(manager as any, 'fitPane').mockResolvedValue(undefined);
			(manager as any)._recomputeViewport = vi.fn();
			(manager as any)._invalidateHost = vi.fn();
			(manager as any)._ensureResizeReleaseListener = vi.fn();

			manager.viewportChanged(PANE);
			manager.viewportChanged(PANE);
			expect((manager as any)._recomputeViewport).toHaveBeenCalledTimes(2);
			expect(fixture.pane.pendingFitTimer).not.toBeNull();
			await vi.advanceTimersByTimeAsync(500);
			expect(fitPane).toHaveBeenCalledWith(fixture.pane, false);

			manager.park(PANE, 'coverage');
			manager.viewportChanged(PANE);
			manager.viewportChanged('missing');
			internal.panes.delete(PANE);
			manager.fitPaneNow(PANE);
			manager.fitPaneNow('missing');
			expect(fitPane).toHaveBeenCalledTimes(1);
		} finally {
			vi.useRealTimers();
		}
	});

	it('covers theme parsing, hidden-workspace detection, and viewport projections', () => {
		const { manager, fixture, internal } = makeManager();
		internal.opts.theme = { background: '#12345678' };
		expect((manager as any)._currentThemeBgRgba()).toEqual(new Uint8Array([0x12, 0x34, 0x56, 0x78]));
		internal.opts.theme = { background: '#abcdef' };
		expect((manager as any)._currentThemeBgRgba()).toEqual(new Uint8Array([0xab, 0xcd, 0xef, 0xff]));
		internal.opts.theme = { background: '#nope00' };
		expect((manager as any)._currentThemeBgRgba()).toEqual(new Uint8Array([0, 0, 0, 0]));
		internal.opts.theme = undefined;
		expect((manager as any)._currentThemeBgRgba()).toEqual(new Uint8Array([0, 0, 0, 0]));

		internal._activeWorkspaceId = 'workspace-b';
		expect((manager as any)._isContainerHidden(fixture.pane)).toBe(true);
		internal._activeWorkspaceId = 'workspace-a';
		expect((manager as any)._isContainerHidden(fixture.pane)).toBe(false);
		internal._activeWorkspaceId = null;
		(fixture.pane.container.getBoundingClientRect as ReturnType<typeof vi.fn>).mockReturnValueOnce({
			width: 0, height: 0,
		});
		expect((manager as any)._isContainerHidden(fixture.pane)).toBe(true);
		(fixture.pane.container.getBoundingClientRect as ReturnType<typeof vi.fn>).mockImplementationOnce(() => {
			throw new Error('layout unavailable');
		});
		expect((manager as any)._isContainerHidden(fixture.pane)).toBe(false);

		const host = { resize: vi.fn(), invalidate: vi.fn(), beginFrame: vi.fn(() => true), endFrame: vi.fn() };
		internal.globalHost = { canvas: fixture.pane.canvas, host };
		(manager as any)._recomputeViewport(fixture.pane);
		expect(fixture.handle.setViewportOffset).toHaveBeenCalled();
		expect(fixture.handle.resize).toHaveBeenCalledWith(800, 400, 1);

		internal.globalHost = null;
		internal._sharedRemoteMode = true;
		(manager as any)._recomputeViewport(fixture.pane);
		expect(fixture.pane.canvas.style.position).toBeUndefined();
		(fixture.pane.container.getBoundingClientRect as ReturnType<typeof vi.fn>).mockReturnValueOnce({
			width: 0, height: 0,
		});
		(manager as any)._recomputeViewport(fixture.pane);
	});

	it('fits raw-byte panes, honors DPR drift, and skips visual-only shared fits', async () => {
		vi.useFakeTimers();
		try {
			const { manager, fixture, internal } = makeManager();
			fixture.pane.localGridAuthority = true;
			fixture.pane.resizeHandler = vi.fn();
			await (manager as any).fitPane(fixture.pane);
			expect(fixture.kernel.resize).toHaveBeenCalledWith(20, 80);
			expect(fixture.pane.resizeHandler).toHaveBeenCalledWith(20, 80, false, false);
			expect(fixture.handle.invalidateAll).toHaveBeenCalled();
			await vi.advanceTimersByTimeAsync(150);
			expect(fixture.handle.invalidateAll).toHaveBeenCalledTimes(2);

			(fixture.handle.configure as ReturnType<typeof vi.fn>).mockReturnValue([11, 21]);
			(window as any).devicePixelRatio = 2;
			await (manager as any).fitPane(fixture.pane);
			expect(fixture.handle.configure).toHaveBeenCalledWith('monospace', 14, 2);

			internal._sharedRemoteMode = true;
			fixture.pane.localGridAuthority = false;
			const resizeCalls = fixture.kernel.resize.mock.calls.length;
			await (manager as any).fitPane(fixture.pane, false);
			expect(fixture.kernel.resize).toHaveBeenCalledTimes(resizeCalls);
		} finally {
			vi.useRealTimers();
		}
	});

	it('covers pointer selection, auto-scroll, and worker failure cleanup', async () => {
		vi.useFakeTimers();
		try {
			const { manager, fixture, internal } = makeManager();
			const target = { setPointerCapture: vi.fn(), closest: vi.fn(() => null) };
			fixture.pane.selectionStartAbs = { row: 2, col: 3 };
			expect((manager as any)._extendPointerSelection(fixture.pane, { row: 4, col: 5 }, {
				shiftKey: true, pointerId: 7, target,
			} as unknown as PointerEvent)).toBe(true);
			expect(fixture.kernel.setSelectionAbs).toHaveBeenCalledWith(2, 3, 16, 5);
			expect(target.setPointerCapture).toHaveBeenCalledWith(7);

		fixture.pane.selecting = true;
		fixture.pane.selectionStartAbs = { row: 1, col: 1 };
		const insideFirstRow = { clientX: 30, clientY: 21, pointerId: 8 } as unknown as PointerEvent;
		(manager as any)._updateAttachAutoScroll(PANE, fixture.pane, insideFirstRow);
		expect(fixture.pane.autoScrollDirection).toBeNull();
		const event = { clientX: 30, clientY: 19, pointerId: 8 } as unknown as PointerEvent;
		(manager as any)._updateAttachAutoScroll(PANE, fixture.pane, { ...event, clientY: 200 });
		(manager as any)._updateAttachAutoScroll(PANE, fixture.pane, event);
		expect(fixture.pane.autoScrollDirection).toBe('up');
		await vi.advanceTimersByTimeAsync(30);
		expect(fixture.kernel.scrollUp).toHaveBeenCalledWith(1);
		fixture.pane.autoScrollDirection = null;
		const insideLastRow = { clientX: 30, clientY: 419, pointerId: 8 } as unknown as PointerEvent;
		(manager as any)._updateAttachAutoScroll(PANE, fixture.pane, insideLastRow);
		expect(fixture.pane.autoScrollDirection).toBeNull();
		const belowLastRow = { clientX: 30, clientY: 421, pointerId: 8 } as unknown as PointerEvent;
		(manager as any)._updateAttachAutoScroll(PANE, fixture.pane, belowLastRow);
		expect(fixture.pane.autoScrollDirection).toBe('down');
		await vi.advanceTimersByTimeAsync(30);
		expect(fixture.kernel.scrollDown).toHaveBeenCalledWith(1);
		fixture.pane.selecting = false;
		await vi.advanceTimersByTimeAsync(30);
		expect(fixture.pane.autoScrollTimer).toBeNull();

	} finally {
		vi.useRealTimers();
	}
	});

	it('covers feed tracing and retained/unpark resource guards', async () => {
		const { manager, fixture, internal } = makeManager();
		const debug = vi.spyOn(console, 'debug').mockImplementation(() => undefined);
		(fixture.pane.linkSpans as any).markDirty = vi.fn();
		manager.onData(PANE, vi.fn());
		(globalThis.localStorage.getItem as ReturnType<typeof vi.fn>).mockImplementation((key: string) =>
			key === 'RIDGE_PTY_TRACE' || key === 'RIDGE_CURSOR_TRACE' ? '1' : null,
		);
		manager.feed(PANE, new Uint8Array(300).fill(65));
		expect(fixture.kernel.feed).toHaveBeenCalled();
		expect(debug).toHaveBeenCalled();
		fixture.kernel.getSelectionText.mockReturnValue('');
		fixture.kernel.encodeKey.mockReturnValue(new Uint8Array([0x03]));
		expect(manager.handleKeyDown(PANE, {
			key: 'c', ctrlKey: true, metaKey: false, altKey: false, shiftKey: false,
		} as KeyboardEvent)).toBe(true);
		expect(debug).toHaveBeenCalledWith(expect.stringContaining('[cursor-trace]'));

		const container = makeContainer();
		internal.globalHost = { canvas: { getBoundingClientRect: fixture.pane.canvas.getBoundingClientRect } as HTMLCanvasElement, host: {} };
		fixture.pane.rendererRetained = false;
		fixture.pane.handle = null;
		const selected = (manager as any)._selectUnparkCanvas(container);
		expect(selected.hostHandle).toBe(internal.globalHost.host);

		fixture.pane.rendererRetained = true;
		fixture.pane.handle = fixture.handle;
		vi.stubGlobal('document', {
			createElement: vi.fn(() => ({
				style: {} as Record<string, string>,
				setAttribute: vi.fn(),
				remove: vi.fn(),
			})),
		});
		internal._makeHandleSerialized = vi.fn(async () => fixture.handle);
		const prepared = await (manager as any)._prepareUnparkResources(container, fixture.pane);
		expect(fixture.handle.free).toHaveBeenCalled();
		expect(prepared.handle).toBe(fixture.handle);

		const staleHandle = { free: vi.fn() };
		(manager as any)._commitUnpark(PANE, container, fixture.pane, {
			canvas: selected.canvas, hostHandle: selected.hostHandle, handle: staleHandle, dpr: 1,
		});
		expect(staleHandle.free).toHaveBeenCalledOnce();
		debug.mockRestore();
	});

	it('covers host fit geometry', async () => {
		vi.useFakeTimers();
		try {
			const { manager, fixture, internal } = makeManager();
			const hostCanvas = fixture.pane.canvas;
			internal.globalHost = { canvas: hostCanvas, host: { beginFrame: vi.fn(() => true), endFrame: vi.fn() } };
			(globalThis.window.getComputedStyle as ReturnType<typeof vi.fn>).mockReturnValue({
				paddingLeft: '4px', paddingRight: '6px', paddingTop: '2px', paddingBottom: '3px',
			});
			expect((manager as any)._measureFit(fixture.pane)).toEqual({ wCss: 790, hCss: 395 });
			await (manager as any).fitPane(fixture.pane, true);
			expect(fixture.handle.setViewportOffset).toHaveBeenCalled();

		} finally {
			vi.useRealTimers();
		}
	});

	it('covers frame dirty-state, sync-output, and idle scheduling guards', async () => {
		vi.useFakeTimers();
		try {
			const { manager, fixture, internal } = makeManager();
			const host = {
				beginFrame: vi.fn(() => true),
				endFrame: vi.fn(),
				needsFullSeed: vi.fn(() => false),
			};
			internal.globalHost = { canvas: fixture.pane.canvas, host };
			internal._sharedRemoteMode = true;
			(fixture.handle as any).isDirty = vi.fn(() => false);
			(fixture.handle as any).nextBlinkDeadlineMs = vi.fn(() => 25);
			const state: any = {
				frameOrder: [fixture.pane],
				dirtyByPane: new Map([[PANE, false]]),
				activeHost: host,
				hostFrameOpen: false,
				surfaceJustWiped: false,
				anyRendered: false,
				minDeadlineMs: Infinity,
				dateNow: Date.now(),
				perfNow: 10,
			};

			expect((manager as any)._hostPaneDirty(fixture.pane, state.dateNow)).toBe(false);
		(fixture.handle.isDirty as ReturnType<typeof vi.fn>).mockImplementationOnce(() => { throw new Error('old host'); });
			expect((manager as any)._hostPaneDirty(fixture.pane, state.dateNow)).toBe(true);
		fixture.pane.handle = null;
			expect((manager as any)._hostPaneDirty(fixture.pane, state.dateNow)).toBe(true);
		fixture.pane.handle = fixture.handle;

			(manager as any)._collectHostDirty([fixture.pane], state.dateNow);
			host.needsFullSeed.mockReturnValueOnce(true);
			internal._hostInvalidatePending = false;
			expect((manager as any)._newRafFrame(20, state.dateNow).surfaceJustWiped).toBe(true);
			expect((manager as any)._ensureHostFrame(state)).toBe(true);
		expect((manager as any)._ensureHostFrame({ ...state, hostFrameOpen: true })).toBe(true);
		expect((manager as any)._ensureHostFrame({ ...state, hostFrameOpen: false, activeHost: null })).toBe(false);

		fixture.kernel.isSyncOutput.mockReturnValue(false);
		fixture.pane.syncStart = 10;
		expect((manager as any)._renderEntryAfterSync(fixture.pane, state)).toBe(true);
		fixture.kernel.isSyncOutput.mockReturnValue(true);
		state.perfNow = 10;
		expect((manager as any)._renderEntryAfterSync(fixture.pane, state)).toBe(false);
		state.perfNow = 1_000_000;
		expect((manager as any)._renderEntryAfterSync(fixture.pane, state)).toBe(true);
		expect((manager as any)._renderEntryAfterSync(fixture.pane, state)).toBe(false);

		expect((manager as any)._entryDirty(fixture.pane, { ...state, dirtyByPane: new Map() })).toBe(true);
		internal.globalHost = null;
		(fixture.handle.isDirty as ReturnType<typeof vi.fn>).mockReturnValueOnce(false);
		expect((manager as any)._entryDirty(fixture.pane, state)).toBe(false);
		(fixture.handle.isDirty as ReturnType<typeof vi.fn>).mockImplementationOnce(() => { throw new Error('dirty probe'); });
		expect((manager as any)._entryDirty(fixture.pane, state)).toBe(true);
		internal.globalHost = { canvas: fixture.pane.canvas, host };

		(manager as any)._paintFrameEntry(fixture.pane, state, false);
		state.surfaceJustWiped = true;
		(manager as any)._paintFrameEntry(fixture.pane, state, false);
		expect(fixture.handle.repaintAll).toHaveBeenCalledOnce();
		state.surfaceJustWiped = false;
		(fixture.handle.render as ReturnType<typeof vi.fn>).mockImplementationOnce(() => { throw new Error('render failed'); });
		(manager as any)._paintFrameEntry(fixture.pane, state, true);
		(manager as any)._updateBlinkDeadline(fixture.pane, state);
		(fixture.handle.nextBlinkDeadlineMs as ReturnType<typeof vi.fn>).mockImplementationOnce(() => { throw new Error('blink probe'); });
		(manager as any)._updateBlinkDeadline(fixture.pane, state);

		internal._activeWorkspaceId = 'other-workspace';
		(manager as any)._renderFrameEntry(fixture.pane, state);
		internal._activeWorkspaceId = null;
		(fixture.handle.render as ReturnType<typeof vi.fn>).mockClear();
		(fixture.handle.repaintAll as ReturnType<typeof vi.fn>).mockClear();
		(host.beginFrame as ReturnType<typeof vi.fn>).mockClear();
		(manager as any)._renderFrameEntry(fixture.pane, { ...state, activeHost: host });
		expect(fixture.handle.render).not.toHaveBeenCalled();
		expect(fixture.handle.repaintAll).not.toHaveBeenCalled();
		expect(host.beginFrame).not.toHaveBeenCalled();

		fixture.pane.wasHiddenLastTick = true;
		vi.spyOn(manager as any, 'fitPane').mockResolvedValue(undefined);
		(manager as any)._renderFrameEntry(fixture.pane, { ...state, activeHost: host });

		state.hostFrameOpen = true;
		(manager as any)._finishHostFrame(state);
		(host.endFrame as ReturnType<typeof vi.fn>).mockImplementationOnce(() => { throw new Error('frame end'); });
		(manager as any)._finishHostFrame(state);
		const startRaf = vi.spyOn(manager as any, 'startRafLoop').mockImplementation(() => undefined);
		internal._hostInvalidatePending = false;
		(manager as any)._scheduleIdleFrame({ ...state, minDeadlineMs: 5 }, vi.fn());
		await vi.advanceTimersByTimeAsync(5);
		expect(startRaf).toHaveBeenCalled();
		expect(internal._hostInvalidatePending).toBe(false);
		(manager as any)._scheduleNextFrame({ ...state, anyRendered: true }, vi.fn());
		(manager as any)._scheduleNextFrame({ ...state, anyRendered: false }, vi.fn());
	} finally {
		vi.useRealTimers();
	}
	});

	it('forces a same-sized shared Remote claim after the resize handler binds', async () => {
		vi.useFakeTimers();
		try {
			const { manager, fixture, internal } = makeManager();
			internal._sharedRemoteMode = true;
			fixture.setRows(20);
			fixture.setCols(80);
			fixture.pane.lastReportedRows = 20;
			fixture.pane.lastReportedCols = 80;
			fixture.pane.resizeHandler = vi.fn().mockResolvedValue(undefined);

			await manager.fitPaneNow(PANE);
			expect(fixture.pane.resizeHandler).not.toHaveBeenCalled();

			await manager.fitPaneNow(PANE, true);
			expect(fixture.kernel.resize).toHaveBeenCalledWith(20, 80);
			expect(fixture.pane.resizeHandler).toHaveBeenCalledWith(20, 80, false, false);
		} finally {
			vi.useRealTimers();
		}
	});

	it('renders only the pane dirtied by a bottom-row scroll', () => {
		const { manager, fixture, internal } = makeManager();
		const sibling = makePane();
		sibling.pane.paneId = 'stable-sibling';
		sibling.pane.canvas = fixture.pane.canvas;
		internal.panes.set(sibling.pane.paneId, sibling.pane);
		internal._activeWorkspaceId = fixture.pane.workspaceId;
		const host = {
			beginFrame: vi.fn(() => true),
			endFrame: vi.fn(),
		};
		internal.globalHost = { canvas: fixture.pane.canvas, host };
		const state: any = {
			frameOrder: [fixture.pane, sibling.pane],
			dirtyByPane: new Map([
				[fixture.pane.paneId, true],
				[sibling.pane.paneId, false],
			]),
			activeHost: host,
			hostFrameOpen: false,
			surfaceJustWiped: false,
			anyRendered: false,
			minDeadlineMs: Infinity,
			dateNow: Date.now(),
			perfNow: performance.now(),
		};

		internal._renderFrameEntry(fixture.pane, state);
		internal._renderFrameEntry(sibling.pane, state);
		internal._finishHostFrame(state);

		expect(fixture.handle.render).toHaveBeenCalledOnce();
		expect(sibling.handle.render).not.toHaveBeenCalled();
		expect(host.beginFrame).toHaveBeenCalledOnce();
		expect(host.endFrame).toHaveBeenCalledOnce();
	});

	it('defers dirty sibling paints after the frame budget and schedules another turn', () => {
		const { manager, fixture, internal } = makeManager();
		const sibling = makePane();
		sibling.pane.paneId = 'budget-sibling';
		sibling.pane.canvas = fixture.pane.canvas;
		internal.panes.set(sibling.pane.paneId, sibling.pane);
		internal._activeWorkspaceId = fixture.pane.workspaceId;
		const host = { beginFrame: vi.fn(() => true), endFrame: vi.fn() };
		internal.globalHost = { canvas: fixture.pane.canvas, host };
		const now = vi.spyOn(performance, 'now').mockReturnValue(20);
		const state: any = {
			frameOrder: [fixture.pane, sibling.pane],
			dirtyByPane: new Map([[PANE, true], [sibling.pane.paneId, true]]),
			activeHost: host,
			hostFrameOpen: false,
			surfaceJustWiped: false,
			anyRendered: false,
			renderDeferred: false,
			renderDeadlineMs: 8,
			minDeadlineMs: Infinity,
			dateNow: Date.now(),
			perfNow: 20,
		};
		const raf = vi.fn();
		vi.stubGlobal('requestAnimationFrame', raf);

		internal._renderFrameEntry(fixture.pane, state);
		internal._renderFrameEntry(sibling.pane, state);
		internal._finishHostFrame(state);
		internal._scheduleNextFrame(state, vi.fn());

		expect(fixture.handle.render).toHaveBeenCalledOnce();
		expect(sibling.handle.render).not.toHaveBeenCalled();
		expect(state.renderDeferred).toBe(true);
		expect(raf).toHaveBeenCalledOnce();
		now.mockRestore();
	});

	it('keeps an 8-workspace 4-split workload GPU-bound to the active workspace', () => {
		const { manager, fixture, internal } = makeManager();
		const fixtures = [fixture];
		fixture.pane.workspaceId = 'workspace-0';
		for (let workspace = 0; workspace < 8; workspace += 1) {
			for (let split = workspace === 0 ? 1 : 0; split < 4; split += 1) {
				const next = workspace === 0 && split === 0 ? fixture : makePane();
				next.pane.paneId = `stress-${workspace}-${split}`;
				next.pane.workspaceId = `workspace-${workspace}`;
				next.pane.canvas = fixture.pane.canvas;
				internal.panes.set(next.pane.paneId, next.pane);
				if (next !== fixture) fixtures.push(next);
			}
		}
		internal._activeWorkspaceId = 'workspace-3';
		internal._focusedPaneByWorkspace.set('workspace-3', 'stress-3-0');
		const host = { beginFrame: vi.fn(() => true), endFrame: vi.fn() };
		internal.globalHost = { canvas: fixture.pane.canvas, host };
		for (const { pane } of fixtures) manager.enqueueFeed(pane.paneId, new Uint8Array([1]));

		const order = internal._renderOrder();
		const feedOrder = internal._feedOrder();
		expect(order).toHaveLength(4);
		expect(order.every((entry: any) => entry.workspaceId === 'workspace-3')).toBe(true);
		expect(feedOrder).toHaveLength(32);
		const collected = internal._collectHostDirty(order, Date.now());
		const state: any = {
			frameOrder: order,
			...collected,
			activeHost: host,
			hostFrameOpen: false,
			surfaceJustWiped: false,
			anyRendered: false,
			renderDeferred: false,
			renderDeadlineMs: Infinity,
			minDeadlineMs: Infinity,
			dateNow: Date.now(),
			perfNow: 10,
		};
		for (const entry of order) internal._renderFrameEntry(entry, state);
		internal._finishHostFrame(state);

		const active = fixtures.filter(({ pane }) => pane.workspaceId === 'workspace-3');
		expect(active).toHaveLength(4);
		expect(active.every(({ handle }) => handle.render.mock.calls.length === 1)).toBe(true);
		expect(fixtures.filter(({ pane }) => pane.workspaceId !== 'workspace-3')
			.every(({ handle }) => handle.render.mock.calls.length === 0)).toBe(true);
		expect(host.beginFrame).toHaveBeenCalledOnce();
		expect(host.endFrame).toHaveBeenCalledOnce();
		void manager;
	});

	it('backs off after a failed host frame instead of spinning RAF', async () => {
		vi.useFakeTimers();
		try {
			const { manager } = makeManager();
			const startRaf = vi.spyOn(manager as any, 'startRafLoop').mockImplementation(() => undefined);
			(manager as any)._scheduleNextFrame({ frameFailed: true }, vi.fn());
			expect(startRaf).not.toHaveBeenCalled();
			await vi.advanceTimersByTimeAsync(49);
			expect(startRaf).not.toHaveBeenCalled();
			await vi.advanceTimersByTimeAsync(1);
			expect(startRaf).toHaveBeenCalledOnce();
		} finally {
			vi.useRealTimers();
		}
	});
});

describe('TerminalManager explicit claim remount', () => {
	it('claimPaneSize remounts the host at the measured pane grid even when last claim matches', async () => {
		const { manager, fixture } = makeManager();
		internalShared(manager);
		fixture.setRows(20);
		fixture.setCols(80);
		fixture.pane.lastReportedRows = 20;
		fixture.pane.lastReportedCols = 80;
		const resizeHandler = vi.fn().mockResolvedValue(undefined);
		manager.onResize(PANE, resizeHandler);

		manager.fitPaneNow(PANE);
		await Promise.resolve();
		expect(resizeHandler).not.toHaveBeenCalled();

		manager.claimPaneSize(PANE);
		await Promise.resolve();
		expect(resizeHandler).toHaveBeenCalledTimes(1);
		expect(resizeHandler).toHaveBeenCalledWith(20, 80, false, false);
		expect(resizeHandler.mock.calls[0][0]).not.toBe(24);
		expect(fixture.handle.invalidateAll).toHaveBeenCalled();
	});

	it('applies an external canonical grid without issuing a resize claim', () => {
		const { manager, fixture } = makeManager();
		const resizeHandler = vi.fn();
		manager.onResize(PANE, resizeHandler);

		manager.applyPaneResize(PANE, 31, 101);

		expect(fixture.kernel.resize).toHaveBeenCalledWith(31, 101);
		expect(fixture.pane.lastReportedRows).toBe(31);
		expect(fixture.pane.lastReportedCols).toBe(101);
		expect(resizeHandler).not.toHaveBeenCalled();
		expect(fixture.handle.invalidateAll).toHaveBeenCalled();
	});
});

function internalShared(manager: TerminalManager): void {
	(manager as unknown as { _sharedRemoteMode: boolean })._sharedRemoteMode = true;
}
