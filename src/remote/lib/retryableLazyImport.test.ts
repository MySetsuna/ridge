import { afterEach, describe, expect, it, vi } from 'vitest';
import { retryableLazyImport } from './retryableLazyImport';

describe('retryable demand loading', () => {
  afterEach(() => { vi.unstubAllGlobals(); });
  it('does no work before demand and shares concurrent/successful requests', async () => {
    const module = { default: 'component' };
    const request = vi.fn(async () => module);
    const load = retryableLazyImport(request);
    expect(request).not.toHaveBeenCalled();
    const first = load();
    expect(load()).toBe(first);
    await expect(first).resolves.toBe(module);
    expect(load()).toBe(first);
    expect(request).toHaveBeenCalledOnce();
  });

  it('can recover after a network rejection and a synchronous loader failure', async () => {
    const request = vi.fn<() => Promise<string>>()
      .mockRejectedValueOnce(new Error('network unavailable'))
      .mockImplementationOnce(() => { throw new Error('loader failed'); })
      .mockResolvedValue('loaded');
    const load = retryableLazyImport(request);
    await expect(load()).rejects.toThrow('network unavailable');
    await expect(load()).rejects.toThrow('loader failed');
    await expect(load()).resolves.toBe('loaded');
    expect(request).toHaveBeenCalledTimes(3);
  });

  it('does not turn a foreign URL in an error into a module import', async () => {
    vi.stubGlobal('location', new URL('https://trusted.test/'));
    const request = vi.fn<() => Promise<string>>()
      .mockRejectedValueOnce(new TypeError('Failed to fetch dynamically imported module: https://foreign.test/payload.js'))
      .mockResolvedValue('loaded');
    const load = retryableLazyImport(request);
    await expect(load()).rejects.toThrow('foreign.test');
    await expect(load()).resolves.toBe('loaded');
    expect(request).toHaveBeenCalledTimes(2);
  });

  it('waits for the failed Vite stylesheet to load before retrying the component', async () => {
    vi.stubGlobal('location', new URL('https://trusted.test/'));
    const stylesheet = { rel: '', href: '', onload: null as (() => void) | null,
      onerror: null as (() => void) | null, remove: vi.fn() };
    const appendChild = vi.fn(() => queueMicrotask(() => stylesheet.onload?.()));
    vi.stubGlobal('document', { createElement: () => stylesheet, head: { appendChild } });
    const request = vi.fn<() => Promise<string>>()
      .mockRejectedValueOnce(new Error('Unable to preload CSS for /assets/view.css'))
      .mockImplementationOnce(async () => {
        expect(appendChild).toHaveBeenCalledOnce();
        return 'styled component';
      });
    const load = retryableLazyImport(request);
    await expect(load()).rejects.toThrow('Unable to preload CSS');
    await expect(load()).resolves.toBe('styled component');
    expect(stylesheet.href).toBe('https://trusted.test/assets/view.css?ridge-retry=1');
    expect(stylesheet.rel).toBe('stylesheet');
  });
});
