import { afterEach, describe, expect, it, vi } from 'vitest';
import { getCurrent, getCurrentWindow } from './window';

afterEach(() => vi.unstubAllGlobals());

function installWindow() {
  const listeners = new Map<string, EventListener>();
  const requestFullscreen = vi.fn(async () => undefined);
  const exitFullscreen = vi.fn(async () => undefined);
  const document = {
    documentElement: { requestFullscreen },
    fullscreenElement: null as object | null,
    exitFullscreen,
  };
  const window = {
    innerWidth: 640,
    innerHeight: 360,
    addEventListener: vi.fn((name: string, fn: EventListener) => listeners.set(name, fn)),
    removeEventListener: vi.fn((name: string, fn: EventListener) => {
      if (listeners.get(name) === fn) listeners.delete(name);
    }),
  };
  vi.stubGlobal('document', document);
  vi.stubGlobal('window', window);
  return { document, window, listeners, requestFullscreen, exitFullscreen };
}

describe('tauri window shim', () => {
  it('keeps a singleton and maps fullscreen transitions', async () => {
    const env = installWindow();
    const current = getCurrentWindow();
    expect(getCurrent()).toBe(current);
    await expect(current.isMaximized()).resolves.toBe(false);
    await expect(current.isFullscreen()).resolves.toBe(false);
    await current.minimize();
    await current.close();
    await current.setTitle('remote');

    await current.maximize();
    expect(env.requestFullscreen).toHaveBeenCalledOnce();
    env.document.fullscreenElement = {};
    await expect(current.isMaximized()).resolves.toBe(true);
    await current.toggleMaximize();
    expect(env.exitFullscreen).toHaveBeenCalledOnce();
    env.document.fullscreenElement = null;
    await current.toggleMaximize();
    expect(env.requestFullscreen).toHaveBeenCalledTimes(2);
  });

  it('ignores rejected fullscreen calls and emits current resize dimensions', async () => {
    const env = installWindow();
    env.requestFullscreen.mockRejectedValueOnce(new Error('denied'));
    await expect(getCurrentWindow().maximize()).resolves.toBeUndefined();
    env.exitFullscreen.mockRejectedValueOnce(new Error('denied'));
    env.document.fullscreenElement = {};
    await expect(getCurrentWindow().unmaximize()).resolves.toBeUndefined();

    const handler = vi.fn();
    const off = await getCurrentWindow().onResized(handler);
    env.listeners.get('resize')?.(new Event('resize'));
    expect(handler).toHaveBeenCalledWith({ payload: { width: 640, height: 360 } });
    off();
    expect(env.window.removeEventListener).toHaveBeenCalledWith('resize', expect.any(Function));
    await expect(getCurrentWindow().onCloseRequested(vi.fn())).resolves.toEqual(expect.any(Function));
  });
});
