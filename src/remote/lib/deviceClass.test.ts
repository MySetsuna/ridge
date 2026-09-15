import { describe, expect, it, vi } from 'vitest';

import { readDeviceClass, watchDeviceClass } from './deviceClass';

// §P0 回归钉：device-class 判定决定手机端 SPA 在桌面/电脑上是否仍露虚拟键盘栏。
// 错误的默认会把虚拟键盘栏永久钉在桌面浏览器上，占位 + 误触 + 让 macOS Cmd 键
// 在 modState 路径之外乱抢焦点。这套单测钉住两条契约：
//   - `(pointer: fine) ∧ (hover: hover)` → 有实体键盘，不该再叠虚拟键盘；
//   - 其余组合 → 没有实体键盘，虚拟键盘栏必须保留（手机/平板/笔电触屏）。
describe('readDeviceClass', () => {
  type WindowFake = {
    matchMedia: (q: string) => { matches: boolean };
  };
  const setWindow = (queries: Record<string, boolean>): void => {
    const w = globalThis as unknown as { window: WindowFake | undefined };
    w.window = {
      matchMedia: (q: string) => ({ matches: queries[q] ?? false }),
    };
  };

  it('treats pointer:fine + hover:hover as real-keyboard', () => {
    setWindow({ '(pointer: fine)': true, '(hover: hover)': true });
    expect(readDeviceClass().hasRealKeyboard).toBe(true);
  });

  it('treats touch-only as no real keyboard', () => {
    setWindow({ '(pointer: fine)': false, '(hover: hover)': false });
    expect(readDeviceClass().hasRealKeyboard).toBe(false);
  });

  it('does not flip to real keyboard on pointer:fine alone', () => {
    // iPad 配妙控键盘 / 笔电带触屏：pointer:fine=true 但 hover 未必 true；
    // 必须两个都为真才认。保守一面，避免在触屏主导设备上误关虚拟键盘。
    setWindow({ '(pointer: fine)': true, '(hover: hover)': false });
    expect(readDeviceClass().hasRealKeyboard).toBe(false);
  });

  it('returns false when window or matchMedia is unavailable', () => {
    const w = globalThis as unknown as { window: undefined };
    const saved = (globalThis as unknown as { window?: unknown }).window;
    w.window = undefined;
    try {
      expect(readDeviceClass().hasRealKeyboard).toBe(false);
    } finally {
      (globalThis as unknown as { window?: unknown }).window = saved;
    }

    (globalThis as unknown as { window: { matchMedia?: unknown } }).window = {
      matchMedia: undefined,
    };
    expect(readDeviceClass().hasRealKeyboard).toBe(false);
  });
});

describe('watchDeviceClass', () => {
  it('subscribes to both media queries and unsubscribes cleanly', () => {
    const listeners: Array<() => void> = [];
    const mq = (matches: boolean) => {
      const mql = {
        matches,
        addEventListener: (_: string, cb: () => void) => listeners.push(cb),
        removeEventListener: (_: string, cb: () => void) => {
          const idx = listeners.indexOf(cb);
          if (idx >= 0) listeners.splice(idx, 1);
        },
      };
      return mql;
    };
    const windowFake = {
      matchMedia: (q: string) => mq(q === '(pointer: fine)' || q === '(hover: hover)'),
    };
    (globalThis as unknown as { window: typeof windowFake }).window = windowFake;

    const cb = vi.fn();
    const stop = watchDeviceClass(cb);
    expect(listeners).toHaveLength(2);

    listeners.forEach((f) => f());
    expect(cb).toHaveBeenCalledTimes(2);

    stop();
    expect(listeners).toHaveLength(0);
  });

  it('falls back to addListener/removeListener on legacy browsers', () => {
    const listeners: Array<() => void> = [];
    const mq = () => ({
      matches: true,
      addListener: (cb: () => void) => listeners.push(cb),
      removeListener: (cb: () => void) => {
        const idx = listeners.indexOf(cb);
        if (idx >= 0) listeners.splice(idx, 1);
      },
    });
    (globalThis as unknown as { window: { matchMedia: typeof mq } }).window = {
      matchMedia: mq,
    };
    const cb = vi.fn();
    const stop = watchDeviceClass(cb);
    listeners.forEach((f) => f());
    expect(cb).toHaveBeenCalledTimes(2);
    stop();
    expect(listeners).toHaveLength(0);
  });

  it('returns a no-op unsubscribe in SSR / no-window environments', () => {
    const saved = (globalThis as unknown as { window?: unknown }).window;
    (globalThis as unknown as { window: undefined }).window = undefined;
    try {
      const stop = watchDeviceClass(() => {});
      expect(typeof stop).toBe('function');
      stop(); // 不应抛
    } finally {
      (globalThis as unknown as { window?: unknown }).window = saved;
    }
  });
});
