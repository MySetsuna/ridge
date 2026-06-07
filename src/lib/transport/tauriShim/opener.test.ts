import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { openUrl, openPath, revealItemInDir } from './opener';

// vitest 在 node 环境跑（vitest.config.ts: environment 'node'），默认无 window，
// 故手动桩一个最小 window，断言 openUrl 把外链委托给 window.open。

describe('tauriShim/opener', () => {
  const realWindow = (globalThis as { window?: unknown }).window;
  let openSpy: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    openSpy = vi.fn();
    (globalThis as unknown as { window: { open: typeof openSpy } }).window = {
      open: openSpy,
    };
  });

  afterEach(() => {
    if (realWindow === undefined) {
      delete (globalThis as { window?: unknown }).window;
    } else {
      (globalThis as { window?: unknown }).window = realWindow;
    }
    vi.restoreAllMocks();
  });

  it('openUrl 用 window.open 在新标签打开外链（noopener）', async () => {
    // Arrange
    const url = 'https://example.com/foo';

    // Act
    await openUrl(url);

    // Assert
    expect(openSpy).toHaveBeenCalledTimes(1);
    expect(openSpy).toHaveBeenCalledWith(url, '_blank', 'noopener');
  });

  it('openUrl 把 URL 实例转为字符串后再打开', async () => {
    // Arrange
    const url = new URL('https://example.com/bar');

    // Act
    await openUrl(url);

    // Assert
    expect(openSpy).toHaveBeenCalledWith('https://example.com/bar', '_blank', 'noopener');
  });

  it('openUrl 忽略 openWith 参数（浏览器无法指定 OS 应用）', async () => {
    // Act
    await openUrl('https://example.com', 'firefox');

    // Assert
    expect(openSpy).toHaveBeenCalledWith('https://example.com', '_blank', 'noopener');
  });

  it('openPath / revealItemInDir 在浏览器降级为 no-op（仅 warn，不调 window.open）', async () => {
    // Arrange
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    // Act
    await openPath('/host/path/file.txt');
    await revealItemInDir('/host/path/file.txt');

    // Assert
    expect(openSpy).not.toHaveBeenCalled();
    expect(warnSpy).toHaveBeenCalledTimes(2);
  });
});
