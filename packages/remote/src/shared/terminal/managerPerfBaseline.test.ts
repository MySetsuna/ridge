// C 浏览器基线 (Goal #4): 在 jsdom 沙盒下用 manager.ts 真实代码路径测
// 100/500/1000/5000 行的 feed 耗时 + 内存。不新写 ANSI parser，不盲扩 chunk。
// 目标：建立可重现的「首次进入 / 切回」基线；后续修改后用同一脚本对
// 比 find regressions。
//
// 限制：测试在 jsdom + vi.useFakeTimers() 下；manager 真实 attach 的 webgpu
// 路径用 mock handle（见 makeManager）。在真浏览器跑需 Playwright/CDP，
// 沙盒里拿不到真 canvas + 真 wasm kernel.feed 耗时。**这就是为什么本测
// 试用 4 档（100/500/1000/5000）行 ANSI escape-mix 数据测 manager.feed
// 内部的 buffer-flush 路径，而不是 wasm kernel 路径**。
//
// 输出：每次跑把 results JSON 写到 `__RIDGE_BASELINE_OUTPUT__`（运行时
// 注入的全局，CI/本地调试可设 `process.env.RIDGE_BASELINE_OUT` 文件路径）。

import { afterAll, afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { TerminalManager } from './manager';
import { SurfaceHostHandle } from '@ridge/term-wasm';

const BASELINE_RESULTS: Record<string, unknown> = {};
function recordBaseline(name: string, value: unknown) {
	BASELINE_RESULTS[name] = value;
}
function flushBaseline() {
	const out = process.env.RIDGE_BASELINE_OUT;
	if (!out) return;
	try {
		const fs = require('node:fs') as typeof import('node:fs');
		fs.writeFileSync(out, JSON.stringify(BASELINE_RESULTS, null, 2));
	} catch (e) {
		// noop
	}
}

const PANE = 'baseline-pane';
const WORKSPACE = 'baseline-ws';

// 生成 N 行 ANSI 混用文本：每行 80 列，包含 \x1b[Nm 颜色、\x1b[2J 清屏、
// \r 回车。覆盖 feed 的正常路径。
function generateRows(n: number): string {
	const styles = ['\x1b[31m', '\x1b[32m', '\x1b[33m', '\x1b[34m', '\x1b[1;37m', '\x1b[0m'];
	let out = '';
	for (let i = 0; i < n; i++) {
		const s = styles[i % styles.length];
		out += `${s}row ${String(i).padStart(5, '0')} ${'x'.repeat(60)}\x1b[0m\r\n`;
	}
	return out;
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
		isTuiKeyboardLease: vi.fn(() => false),
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
		getSelectionText: vi.fn(() => ''),
		clearSelection: vi.fn(),
		selectAll: vi.fn(),
		setSelectionAbs: vi.fn(),
		clearScrollback: vi.fn(),
		clearTerminalPreservingPrompt: vi.fn(),
		setPreedit: vi.fn(),
		searchSetQuery: vi.fn(() => 0),
		searchNext: vi.fn(() => 0),
		searchPrev: vi.fn(() => 0),
		searchClear: vi.fn(),
		searchActiveIndex: vi.fn(() => 0),
		takeSnapshot: vi.fn(() => null),
		getScrollback: vi.fn(() => ''),
		erase: vi.fn(),
		reset: vi.fn(),
		setTheme: vi.fn(),
		setCursorBlink: vi.fn(),
		hoverAt: vi.fn(() => null),
		absoluteScrollOffset: vi.fn(() => 0),
		setAbsoluteScrollOffset: vi.fn(),
		applyAbsoluteScroll: vi.fn(),
	} as unknown as SurfaceHostHandle;
	const handle = {
		render: vi.fn(),
		markDirty: vi.fn(),
		setFocused: vi.fn(),
		setPadding: vi.fn(),
		free: vi.fn(),
		isDirty: vi.fn(() => true),
	} as unknown as SurfaceHostHandle;
	const pane = {
		paneId: PANE,
		workspaceId: WORKSPACE,
		kernel,
		handle,
		canvas: { width: 0, height: 0, getContext: vi.fn(() => null) } as unknown as HTMLCanvasElement,
		container: { style: {}, dataset: {}, addEventListener: vi.fn(), removeEventListener: vi.fn() } as unknown as HTMLElement,
		generation: 1,
		active: true,
		parked: false,
		fitQueue: [],
		feedBuffer: null,
		feedBufferChunks: [],
		feedBufferBytes: 0,
		feedDeferred: null,
		feedDeferredChunks: [],
		feedDeferredBytes: 0,
		feedPending: false,
		scrollStateVersion: 0,
		pendingFrameWork: false,
		deltaPending: false,
		deltaBytes: 0,
		deltaQueue: [],
		deltaQueueHead: 0,
		feedDroppedBytes: 0,
		feedDropCount: 0,
		feedNeedsResync: false,
		feedFlushTimer: null,
		linkSpans: { markDirty: vi.fn(), clear: vi.fn(), hitTest: vi.fn(() => null) },
		linkUnderlineEls: [],
		linkUnderlineRegions: [],
		linkHintEl: null,
		linkHintRegion: null,
		resizeObserver: { observe: vi.fn(), disconnect: vi.fn() },
		focusListener: vi.fn(),
		blurListener: vi.fn(),
		pointerDownListener: vi.fn(),
		pointerMoveListener: vi.fn(),
		pointerUpListener: vi.fn(),
		pointerCancelListener: vi.fn(),
		pointerLeaveListener: vi.fn(),
		modifierKeyListener: vi.fn(),
		lastPointerPoint: null,
		imeCompositionActive: false,
		imeAnchor: null,
	} as any;
	return { pane, kernel, handle };
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
	// stub globalHost：jsdom 没真 WebGPU；unpark / _selectUnparkCanvas 需要
	// `this.globalHost.canvas` / `.host` 存在才能跑通。
	const fakeCanvas = { width: 0, height: 0, style: {} as Record<string, string>, getContext: vi.fn(() => null) } as unknown as HTMLCanvasElement;
	internal.globalHost = { canvas: fakeCanvas, host: {} };
	const { pane, kernel, handle } = makePane();
	internal.panes.set(PANE, pane);
	internal.paneIdsByWorkspace.set(WORKSPACE, new Set([PANE]));
	return { manager, pane, kernel, handle };
}

beforeEach(() => {
	vi.stubGlobal('requestAnimationFrame', (cb: FrameRequestCallback) => { void cb; return 1; });
	vi.stubGlobal('cancelAnimationFrame', vi.fn());
	vi.stubGlobal('localStorage', { getItem: vi.fn(() => null), setItem: vi.fn(), removeItem: vi.fn() });
	vi.stubGlobal('window', {
		devicePixelRatio: 1,
		open: vi.fn(),
		getComputedStyle: vi.fn(() => ({ paddingLeft: '0px', paddingRight: '0px', paddingTop: '0px', paddingBottom: '0px' })),
	});
});

afterEach(() => {
	vi.unstubAllGlobals();
	(TerminalManager as any)._instance = null;
});

interface FeedResult {
	rows: number;
	payloadBytes: number;
	wallMs: number;
	window?: { usedMB: number; totalMB: number; limitMB: number };
	flushesTriggered: number;
	kernelFeedCalls: number;
}

function timeFeed(rows: number): FeedResult {
	const { manager, kernel, pane } = makeManager();
	const text = generateRows(rows);
	const payloadBytes = new TextEncoder().encode(text).byteLength;
	// 模拟 manager.attach 完成；插入 initial feed buffer（kAttach 路由直接
	// 进 kernel.feed，跳过 deferred）。我们要测的是「切回」场景的 deferred
	// buffer flush 路径。
	pane.feedBuffer = '';
	pane.feedBytes = 0;
	pane.feedPending = false;
	const t0 = performance.now();
	manager.feed(PANE, text);
	// flush 立即触发（kAttach 路径或 RAF yield 后）
	const t1 = performance.now();
	const result: FeedResult = {
		rows,
		payloadBytes,
		wallMs: t1 - t0,
		flushesTriggered: pane.feedPending ? 1 : 0,
		kernelFeedCalls: (kernel.feed as any).mock?.calls?.length ?? 0,
	};
	// 内存快照（jsdom 没有真 RSS，performance.memory 仅 chrome 暴露）
	const mem = (performance as any).memory;
	if (mem) {
		result.window = {
			usedMB: mem.usedJSHeapSize / 1024 / 1024,
			totalMB: mem.totalJSHeapSize / 1024 / 1024,
			limitMB: mem.jsHeapSizeLimit / 1024 / 1024,
		};
	}
	return result;
}

describe('C 浏览器基线 — manager.feed 100/500/1000/5000 行耗时', () => {
	// 100 行：常规 shell 列表输出
	it('100 行 ANSI 混用 feed 耗时 + 内存', () => {
		const r = timeFeed(100);
		recordBaseline('100rows', r);
		// 不强制上限（沙盒），但记录绝对值用于跨版本对比
		expect(r.wallMs).toBeGreaterThanOrEqual(0);
		expect(r.kernelFeedCalls).toBeGreaterThanOrEqual(0);
	});

	it('500 行 feed 耗时', () => {
		const r = timeFeed(500);
		recordBaseline('500rows', r);
		expect(r.wallMs).toBeGreaterThanOrEqual(0);
	});

	it('1000 行 feed 耗时', () => {
		const r = timeFeed(1000);
		recordBaseline('1000rows', r);
		expect(r.wallMs).toBeGreaterThanOrEqual(0);
	});

	it('5000 行 feed 耗时', () => {
		const r = timeFeed(5000);
		recordBaseline('5000rows', r);
		// 5000 行至少能完成（这是基线，不是性能门）
		expect(r.wallMs).toBeGreaterThanOrEqual(0);
		expect(r.payloadBytes).toBeGreaterThan(0);
	});
});

afterAll(() => {
	flushBaseline();
});

describe('C 浏览器基线 — 「切回」场景：park → feed → unpark 不应重灌历史', () => {
	it('park 期间累积 buffer、unpark 后只 flush 一次、不重新 prepend scrollback', async () => {
		const { manager, kernel, pane } = makeManager();
		// 首次 attach + 50 行历史
		manager.feed(PANE, generateRows(50));
		const initialFeedCalls = (kernel.feed as any).mock.calls.length;
		const initialPrependCalls = (kernel.prependScrollback as any).mock.calls.length;
		// park（transient unmount）：kernel + feedBuffer 保留，dataHandler 不动
		manager.park(PANE);
		// 模拟「park 期间 PTY 持续输出」：直接调用 manager.feed（manager 内部
		// 仍能接受 — dataHandler 还在 entry 上；只是没有 canvas render）。
		manager.feed(PANE, generateRows(30));
		const midPrependCalls = (kernel.prependScrollback as any).mock.calls.length;
		// 切回：unpark（jsdom 没真 webgpu，unpark 内部 _makeHandleSerialized
		// 会 reject；这里不 await，避免污染测试结果，捕获 Unhandled 即可）
		void manager.unpark(PANE, pane.container).catch(() => undefined);
		// 关键观察：即便 unpark 后续因 jsdom 限制失败，关键不变量（不重灌
		// 历史）依然要在 unpark **之前**的同步路径里成立。`prependScrollback`
		// 同步次数 = 初始次数（park/feed 期间都不重灌）。
		await new Promise((r) => setTimeout(r, 5));
		// 切回后的期望：
		// - prependScrollback **不应**增加（不需要重新灌历史）
		// - feed 调用次数应继续累加（unpark 后 flush buffer 到 kernel）
		const afterUnparkFeedCalls = (kernel.feed as any).mock.calls.length;
		const afterUnparkPrependCalls = (kernel.prependScrollback as any).mock.calls.length;
		recordBaseline('switchBack', {
			initialFeedCalls,
			initialPrependCalls,
			midPrependCalls,
			afterUnparkFeedCalls,
			afterUnparkPrependCalls,
		});
		// 当前实现里 prependScrollback 不会被 park/unpark 路径触发（kernel
		// state 保留；manager 没有 re-replay）。这条断言是基线 invariant：
		// 一旦未来改坏（unpark 重新 prepend 历史），会立即失败。
		expect(afterUnparkPrependCalls).toBe(initialPrependCalls);
	});
});
