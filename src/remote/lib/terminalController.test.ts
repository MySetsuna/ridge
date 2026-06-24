/**
 * terminalController.test.ts
 *
 * 两类修复的确定性单测：
 *  1. 选区坐标：scrolled 状态下 startSelection 的 anchor 须和 extendSelection 的
 *     end 在同一绝对行基准（均加 scrollOffset）。
 *  2. resize/fit：容器像素 + cell 宽高算出正确的 cols/rows 并调用 kernel.resize()。
 *
 * 完全隔离：mock @ridge/term-wasm 以及所有 SvelteKit/$lib 导入，不连真实 host。
 */
import { describe, it, expect, vi, afterEach } from 'vitest';

// ── hoisted mock refs ──────────────────────────────────────────────────────────
const { mockInit, mockKernelInstance, mockRenderInstance } = vi.hoisted(() => {
  // Fake kernel instance — tracks calls to setSelectionAbs and resize
  const mockKernelInstance = {
    scrollbackLen: vi.fn(() => 0),
    scrollOffset: vi.fn(() => 0),
    clearSelection: vi.fn(),
    setSelectionAbs: vi.fn(),
    hasSelection: vi.fn(() => false),
    getSelectionText: vi.fn(() => null),
    resize: vi.fn(),
    feed: vi.fn(),
    takePendingResponse: vi.fn(() => new Uint8Array(0)),
    encodePaste: vi.fn(() => new Uint8Array(0)),
    encodeKey: vi.fn(() => new Uint8Array(0)),
    encodeMouse: vi.fn(() => new Uint8Array(0)),
    isMouseReporting: vi.fn(() => false),
    scrollUp: vi.fn(),
    scrollDown: vi.fn(),
    scrollToBottom: vi.fn(),
    clearScrollback: vi.fn(),
    applyDeltaFrame: vi.fn(),
    searchSetQuery: vi.fn(() => 0),
    searchNext: vi.fn(() => 4294967295),
    searchPrev: vi.fn(() => 4294967295),
    cursorRow: vi.fn(() => 0),
    cursorCol: vi.fn(() => 0),
    free: vi.fn(),
  };

  // Fake renderHandle instance — configure() returns [cellW, cellH]
  const mockRenderInstance = {
    applyDefaultTheme: vi.fn(),
    applyTheme: vi.fn(),
    resize: vi.fn(),
    configure: vi.fn(() => [8, 16]),
    render: vi.fn(),
    setFocused: vi.fn(),
    nextBlinkDeadlineMs: vi.fn(() => 1000),
    free: vi.fn(),
  };

  const mockInit = vi.fn(() => Promise.resolve());

  return { mockInit, mockKernelInstance, mockRenderInstance };
});

// ── mock @ridge/term-wasm ──────────────────────────────────────────────────────
// TerminalKernel is instantiated via `new`, so the mock must be a real constructor.
// RenderHandle.newWithWebgpuFirst and SurfaceHostHandle.init are static methods.
vi.mock('@ridge/term-wasm', () => {
  // TerminalKernel mock: a proper constructor function returning the shared fake instance
  function TerminalKernel() {
    return mockKernelInstance;
  }

  // SurfaceHostHandle mock: static init returns a fake handle with all methods
  class SurfaceHostHandle {
    static init() {
      const handle = {
        clone: () => null,
        free: vi.fn(),
        beginFrame: vi.fn(() => false),
        endFrame: vi.fn(),
        resize: vi.fn(),
        invalidate: vi.fn(),
      };
      return Promise.resolve(handle);
    }
  }

  // RenderHandle mock: static newWithWebgpuFirst returns the render instance
  class RenderHandle {
    static newWithWebgpuFirst() {
      return Promise.resolve(mockRenderInstance);
    }
  }

  return {
    default: () => mockInit(),
    TerminalKernel,
    RenderHandle,
    SurfaceHostHandle,
  };
});

// ── mock wasm ?url import ──────────────────────────────────────────────────────
vi.mock('@ridge/term-wasm/ridge_term_bg.wasm?url', () => ({ default: '/fake.wasm' }));

// ── mock $lib imports ──────────────────────────────────────────────────────────
vi.mock('$lib/terminal/fontStack', () => ({
  REMOTE_TERM_FONT: 'monospace',
  withEmojiFallback: (f: string) => f || 'monospace',
}));
vi.mock('$lib/terminal/flagEmojiSupport', () => ({
  ensureFlagFont: () => false,
}));

// ── globals needed by TerminalController ──────────────────────────────────────
// node environment does not have requestAnimationFrame; stub it.
let rafCounter = 0;
globalThis.requestAnimationFrame = vi.fn((_cb: FrameRequestCallback) => {
  return ++rafCounter;
});
globalThis.cancelAnimationFrame = vi.fn();
// window.devicePixelRatio is read in fitPane()
Object.defineProperty(globalThis, 'window', {
  value: { devicePixelRatio: 1 },
  writable: true,
  configurable: true,
});
// document is used in setupVisibilityHandler()
if (typeof globalThis.document === 'undefined') {
  Object.defineProperty(globalThis, 'document', {
    value: {
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      visibilityState: 'visible',
      hidden: false,
    },
    writable: true,
    configurable: true,
  });
}

// ── subject under test ─────────────────────────────────────────────────────────
import { TerminalController } from './terminalController';

// Helper: build a fake canvas + container with controllable pixel dimensions
function makeElements(containerW = 800, containerH = 400) {
  const canvas = {
    width: 0,
    height: 0,
    style: { width: '', height: '' },
    getBoundingClientRect: vi.fn(() => ({
      left: 0,
      top: 0,
      right: containerW,
      bottom: containerH,
    })),
  } as unknown as HTMLCanvasElement;

  const container = {
    clientWidth: containerW,
    clientHeight: containerH,
  } as unknown as HTMLDivElement;

  return { canvas, container };
}

// Helper: create a TerminalController, resetting the static `initialized` flag
// so that the WASM init mock is always exercised.
async function makeController(containerW = 800, containerH = 400): Promise<TerminalController> {
  (TerminalController as unknown as { initialized: boolean }).initialized = false;
  const { canvas, container } = makeElements(containerW, containerH);
  return TerminalController.create(canvas, container, {});
}

// ── teardown ───────────────────────────────────────────────────────────────────
afterEach(() => {
  vi.clearAllMocks();
  rafCounter = 0;
  (TerminalController as unknown as { initialized: boolean }).initialized = false;
});

// ─────────────────────────────────────────────────────────────────────────────
// GROUP 1: 选区坐标修复
// ─────────────────────────────────────────────────────────────────────────────
describe('Selection coordinate fix — anchor must use the same absolute basis as end', () => {
  /**
   * 核心回归测试：
   * - 模拟终端已滚动：scrollbackLen()>0, scrollOffset()=5
   * - 用户在视口第 2 行（viewport-relative row=2）按下选区起点
   * - 再把选区拖到视口第 4 行（viewport-relative row=4）
   *
   * 修复后：
   *   anchorAbsRow = 2 + 5 = 7
   *   endAbsRow    = 4 + 5 = 9
   *   setSelectionAbs(7, col, 9, col) ← 两端都是绝对行
   *
   * 修复前（bug）：
   *   anchorAbsRow = 2          ← 未加 scrollOffset
   *   endAbsRow    = 4 + 5 = 9
   *   setSelectionAbs(2, col, 9, col) ← 坐标系不一致
   *
   * 该测试在 bug 代码下必然失败（anchor 为 2 而非 7）。
   */
  it('adds scrollOffset to anchor row when terminal is scrolled (regression for bug)', async () => {
    const ctrl = await makeController();

    // Simulate a scrolled terminal: scrollback exists and viewport is offset
    mockKernelInstance.scrollbackLen.mockReturnValue(100);
    mockKernelInstance.scrollOffset.mockReturnValue(5);

    const VIEWPORT_ANCHOR_ROW = 2;
    const VIEWPORT_END_ROW = 4;
    const COL = 10;
    const SCROLL_OFFSET = 5;

    ctrl.startSelection(VIEWPORT_ANCHOR_ROW, COL);
    ctrl.extendSelection(VIEWPORT_END_ROW, COL);

    expect(mockKernelInstance.setSelectionAbs).toHaveBeenCalledTimes(1);
    const [anchorRow, anchorCol, endRow, endCol] = mockKernelInstance.setSelectionAbs.mock.calls[0];

    // Both rows must be absolute (viewport-relative + scrollOffset)
    expect(anchorRow).toBe(VIEWPORT_ANCHOR_ROW + SCROLL_OFFSET); // 7, NOT 2
    expect(endRow).toBe(VIEWPORT_END_ROW + SCROLL_OFFSET);       // 9
    expect(anchorCol).toBe(COL);
    expect(endCol).toBe(COL);
  });

  /**
   * 未滚动状态（scrollOffset=0）：anchor 和 end 都保持视口行号，行为不变。
   */
  it('does not add scrollOffset when terminal is at bottom (scrollOffset=0)', async () => {
    const ctrl = await makeController();

    mockKernelInstance.scrollbackLen.mockReturnValue(100);
    mockKernelInstance.scrollOffset.mockReturnValue(0);

    ctrl.startSelection(3, 5);
    ctrl.extendSelection(6, 8);

    const [anchorRow, , endRow] = mockKernelInstance.setSelectionAbs.mock.calls[0];
    expect(anchorRow).toBe(3); // no scrollOffset added
    expect(endRow).toBe(6);
  });

  /**
   * 无滚动内容（scrollbackLen=0）：也不加 scrollOffset。
   */
  it('does not add scrollOffset when scrollbackLen is 0', async () => {
    const ctrl = await makeController();

    mockKernelInstance.scrollbackLen.mockReturnValue(0);
    mockKernelInstance.scrollOffset.mockReturnValue(5); // nonsense state — should be guarded

    ctrl.startSelection(1, 0);
    ctrl.extendSelection(3, 0);

    const [anchorRow, , endRow] = mockKernelInstance.setSelectionAbs.mock.calls[0];
    expect(anchorRow).toBe(1);
    expect(endRow).toBe(3);
  });

  /**
   * 连续拖拽多次 extendSelection 时，anchor 始终固定在第一次 startSelection 的绝对行。
   */
  it('keeps anchor fixed across multiple extendSelection calls', async () => {
    const ctrl = await makeController();

    mockKernelInstance.scrollbackLen.mockReturnValue(50);
    mockKernelInstance.scrollOffset.mockReturnValue(10);

    ctrl.startSelection(2, 0); // absAnchor = 2 + 10 = 12

    ctrl.extendSelection(3, 5);
    ctrl.extendSelection(5, 5);
    ctrl.extendSelection(7, 5);

    expect(mockKernelInstance.setSelectionAbs).toHaveBeenCalledTimes(3);
    for (const call of mockKernelInstance.setSelectionAbs.mock.calls) {
      expect(call[0]).toBe(12); // anchor always 12
    }
  });

  /**
   * 验证 clientToCell → startSelection 路径（pointer 事件路径）。
   * clientToCell 返回视口相对行；startSelection 应将其转为绝对行。
   */
  it('clientToCell viewport row fed into startSelection is converted to absolute', async () => {
    // configure returns [8, 16], so cellW=8, cellH=16
    // container 800x400 → cols=100, rows=25
    const ctrl = await makeController(800, 400);

    mockKernelInstance.scrollbackLen.mockReturnValue(200);
    mockKernelInstance.scrollOffset.mockReturnValue(8);

    // getBoundingClientRect returns {left:0, top:0} → x=clientX, y=clientY
    // row = floor(48/16)=3, col = floor(100/8)=12
    const cell = ctrl.clientToCell(100, 48);
    expect(cell).toEqual({ row: 3, col: 12 });

    ctrl.startSelection(cell!.row, cell!.col);
    ctrl.extendSelection(5, 12);

    const [anchorRow] = mockKernelInstance.setSelectionAbs.mock.calls[0];
    expect(anchorRow).toBe(3 + 8); // 11 = viewport row 3 + scrollOffset 8
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// GROUP 2: resize / fitPane 测试
// ─────────────────────────────────────────────────────────────────────────────
describe('fitPane / requestResize — cols/rows computed from container pixels + cell size', () => {
  /**
   * 基本正确性：800×400 容器，cellW=8 cellH=16 → cols=100, rows=25
   * kernel.resize 应被调用恰好一次（来自 create() 内的 fitPane()）。
   */
  it('computes cols and rows from container dimensions and cell size on creation', async () => {
    // configure returns [8, 16] (default)
    mockRenderInstance.configure.mockReturnValue([8, 16]);

    await makeController(800, 400);

    // kernel.resize is called from fitPane inside create()
    expect(mockKernelInstance.resize).toHaveBeenCalledWith(25, 100); // rows=25, cols=100
  });

  /**
   * 不同容器尺寸：320×240，cellW=8 cellH=16 → cols=40, rows=15
   */
  it('correctly computes cols=40 rows=15 for 320x240 container with cellW=8 cellH=16', async () => {
    mockRenderInstance.configure.mockReturnValue([8, 16]);

    await makeController(320, 240);

    expect(mockKernelInstance.resize).toHaveBeenCalledWith(15, 40);
  });

  /**
   * requestResize 带防抖：调用后等待 RESIZE_DEBOUNCE_MS (100ms) 才触发 fitPane。
   * 用 vi.useFakeTimers({ toFake }) 控制时钟；先用 640×320 创建，再改容器为 800×400
   * 保证尺寸发生变化，从而触发 kernel.resize（fitPane 只在 cols/rows 变化时才 resize）。
   */
  it('requestResize debounces and eventually calls kernel.resize with correct dims', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    try {
      mockRenderInstance.configure.mockReturnValue([8, 16]);

      // Create with 640×320 → cols=80, rows=20
      const { canvas, container } = makeElements(640, 320);
      (TerminalController as unknown as { initialized: boolean }).initialized = false;
      const ctrl = await TerminalController.create(canvas, container, {});
      mockKernelInstance.resize.mockClear();

      // Simulate container resize to 800×400
      (container as unknown as { clientWidth: number; clientHeight: number }).clientWidth = 800;
      (container as unknown as { clientWidth: number; clientHeight: number }).clientHeight = 400;

      ctrl.requestResize();
      // Before debounce fires, no additional resize
      expect(mockKernelInstance.resize).not.toHaveBeenCalled();

      vi.advanceTimersByTime(100);
      expect(mockKernelInstance.resize).toHaveBeenCalledWith(25, 100);
    } finally {
      vi.useRealTimers();
    }
  });

  /**
   * 多次快速 requestResize 只触发一次 fitPane（防抖合并）。
   */
  it('coalesces rapid requestResize calls into a single fitPane', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    try {
      mockRenderInstance.configure.mockReturnValue([8, 16]);

      // Create with 640×320 → cols=80, rows=20, then resize container
      const { canvas, container } = makeElements(640, 320);
      (TerminalController as unknown as { initialized: boolean }).initialized = false;
      const ctrl = await TerminalController.create(canvas, container, {});
      mockKernelInstance.resize.mockClear();

      (container as unknown as { clientWidth: number; clientHeight: number }).clientWidth = 800;
      (container as unknown as { clientWidth: number; clientHeight: number }).clientHeight = 400;

      ctrl.requestResize();
      ctrl.requestResize();
      ctrl.requestResize();

      vi.advanceTimersByTime(100);
      expect(mockKernelInstance.resize).toHaveBeenCalledTimes(1);
      expect(mockKernelInstance.resize).toHaveBeenCalledWith(25, 100);
    } finally {
      vi.useRealTimers();
    }
  });

  /**
   * requestResizeImmediate 不走防抖，立即触发 fitPane。
   * 为了触发 kernel.resize（变更检测跳过相同尺寸），先用 640×320 创建，再改为 800×400 调用。
   */
  it('requestResizeImmediate calls fitPane synchronously', async () => {
    mockRenderInstance.configure.mockReturnValue([8, 16]);

    // Create with 640×320 → cols=80, rows=20
    const { canvas, container } = makeElements(640, 320);
    (TerminalController as unknown as { initialized: boolean }).initialized = false;
    const ctrl = await TerminalController.create(canvas, container, {});
    expect(mockKernelInstance.resize).toHaveBeenCalledWith(20, 80);
    mockKernelInstance.resize.mockClear();

    // Now simulate container resize to 800×400 → cols=100, rows=25
    (container as unknown as { clientWidth: number; clientHeight: number }).clientWidth = 800;
    (container as unknown as { clientWidth: number; clientHeight: number }).clientHeight = 400;

    ctrl.requestResizeImmediate();
    expect(mockKernelInstance.resize).toHaveBeenCalledWith(25, 100);
  });

  /**
   * cell 尺寸 quantize：TerminalController.quantizeCellSize 把 px 对齐到 1/64。
   * 这是纯函数，直接测。
   */
  it('quantizeCellSize rounds to nearest 1/64', () => {
    expect(TerminalController.quantizeCellSize(8)).toBe(8);
    expect(TerminalController.quantizeCellSize(8.5)).toBe(8.5);
    // 8.1 * 64 = 518.4 → round → 518 / 64 = 8.09375
    expect(TerminalController.quantizeCellSize(8.1)).toBeCloseTo(8.09375, 5);
  });

  /**
   * 如果容器尺寸为 0，fitPane 应提前退出，不调用 kernel.resize。
   */
  it('fitPane bails out when container dimensions are zero', async () => {
    mockRenderInstance.configure.mockReturnValue([8, 16]);
    await makeController(0, 0);
    // kernel.resize should NOT have been called (fitPane returned early due to 0 dims)
    expect(mockKernelInstance.resize).not.toHaveBeenCalled();
  });

  /**
   * cols/rows 下限为 1（Math.max(1, …)）。
   * 极小容器 1×1，cellW=8 cellH=16 → floor(1/8)=0 → max(1,0)=1
   */
  it('clamps cols and rows to minimum of 1 for tiny container', async () => {
    mockRenderInstance.configure.mockReturnValue([8, 16]);
    await makeController(1, 1);
    expect(mockKernelInstance.resize).toHaveBeenCalledWith(1, 1);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// GROUP 3: feedChunked / flushDeferred 分片与预算
// ─────────────────────────────────────────────────────────────────────────────
describe('feedChunked / flushDeferred — chunked feeding with time budget', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('feedChunked splits 48 KiB into three 16 KiB chunks', async () => {
    const ctrl = await makeController();
    const data = new Uint8Array(48 * 1024).fill(65); // 'A', no escape → skip coalesce

    (ctrl as any).feedChunked(data);

    expect(mockKernelInstance.feed).toHaveBeenCalledTimes(3);
    expect(mockKernelInstance.feed.mock.calls[0][0].length).toBe(16 * 1024);
    expect(mockKernelInstance.feed.mock.calls[1][0].length).toBe(16 * 1024);
    expect(mockKernelInstance.feed.mock.calls[2][0].length).toBe(16 * 1024);
    // takePendingResponse called after all chunks processed
    expect(mockKernelInstance.takePendingResponse).toHaveBeenCalledTimes(1);
    // No deferred data left
    expect((ctrl as any).feedDeferred.length).toBe(0);
  });

  it('feedChunked defers remainder when time budget is exceeded', async () => {
    const perfNow = vi.spyOn(performance, 'now');
    let callIdx = 0;
    perfNow.mockImplementation(() => {
      callIdx++;
      return callIdx === 1 ? 0 : 5; // start=0ms → first check at 5ms → >4 → defer
    });

    const ctrl = await makeController();
    const data = new Uint8Array(48 * 1024).fill(65);

    (ctrl as any).feedChunked(data);

    // Only first 16 KiB chunk processed (budget exceeded at chunk 2)
    expect(mockKernelInstance.feed).toHaveBeenCalledTimes(1);
    expect(mockKernelInstance.feed.mock.calls[0][0].length).toBe(16 * 1024);
    // Remaining 32 KiB deferred as one entry
    expect((ctrl as any).feedDeferred.length).toBe(1);
    expect((ctrl as any).feedDeferred[0].length).toBe(32 * 1024);
    // takePendingResponse NOT called — data not fully processed
    expect(mockKernelInstance.takePendingResponse).not.toHaveBeenCalled();
  });

  it('flushDeferred delegates to feedChunked for each deferred entry', async () => {
    const ctrl = await makeController();
    const chunk = new Uint8Array(16 * 1024).fill(65);
    (ctrl as any).feedDeferred.push(chunk);

    const feedChunkedSpy = vi.spyOn(TerminalController.prototype as any, 'feedChunked');
    (ctrl as any).flushDeferred();

    // feedChunked was called with the deferred entry, NOT drained directly via kernel.feed
    expect(feedChunkedSpy).toHaveBeenCalledTimes(1);
    expect(feedChunkedSpy).toHaveBeenCalledWith(chunk);
    // Deferred entry was consumed
    expect((ctrl as any).feedDeferred.length).toBe(0);
  });

  it('flushDeferred is no-op when feedDeferred is empty', async () => {
    const ctrl = await makeController();
    const feedChunkedSpy = vi.spyOn(TerminalController.prototype as any, 'feedChunked');

    (ctrl as any).flushDeferred();

    expect(feedChunkedSpy).not.toHaveBeenCalled();
  });
});
