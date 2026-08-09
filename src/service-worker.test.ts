import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$service-worker', () => ({
  build: ['/assets/app.js', '/assets/app.css'],
  version: 'test-version',
}));

type CacheMock = {
  addAll: ReturnType<typeof vi.fn>;
  match: ReturnType<typeof vi.fn>;
  put: ReturnType<typeof vi.fn>;
};

const state = vi.hoisted(() => ({
  listeners: new Map<string, (event: any) => void>(),
  clients: [{ postMessage: vi.fn() }],
  cacheEntries: new Map<string, CacheMock>(),
  cacheKeys: new Set<string>(),
  caches: undefined as any,
}));

function cacheFor(name: string): CacheMock {
  let cache = state.cacheEntries.get(name);
  if (!cache) {
    cache = {
      addAll: vi.fn(async () => {}),
      match: vi.fn(async (request: RequestInfo) => (
        request === 'version' ? null : undefined
      )),
      put: vi.fn(async () => {}),
    };
    state.cacheEntries.set(name, cache);
  }
  state.cacheKeys.add(name);
  return cache;
}

beforeAll(async () => {
  state.caches = {
    keys: vi.fn(async () => [...state.cacheKeys]),
    open: vi.fn(async (name: string) => cacheFor(name)),
    delete: vi.fn(async (name: string) => state.cacheKeys.delete(name)),
    match: vi.fn(async () => null),
  };
  const sw = {
    clients: { matchAll: vi.fn(async () => state.clients), claim: vi.fn(async () => {}) },
    caches: state.caches,
    addEventListener: vi.fn((type: string, handler: (event: any) => void) => state.listeners.set(type, handler)),
    skipWaiting: vi.fn(async () => {}),
  };
  vi.stubGlobal('self', sw);
  vi.stubGlobal('caches', state.caches);
  vi.stubGlobal('location', { origin: 'https://remote.test' });
  await import('./service-worker');
});

beforeEach(() => {
  state.caches.match.mockReset();
  state.caches.match.mockResolvedValue(null);
  vi.stubGlobal('fetch', vi.fn());
});

function dispatchFetch(request: Request): Promise<Response> | undefined {
  let response: Promise<Response> | undefined;
  state.listeners.get('fetch')?.({ request, respondWith: (value: Promise<Response>) => { response = value; } });
  return response;
}

function swRequest(url: string, mode: Request['mode'] = 'same-origin'): Request {
  return { method: 'GET', mode, url } as Request;
}

describe('web-remote service worker', () => {
  it('installs and activates the versioned shell while removing stale caches', async () => {
    const installWait = vi.fn();
    state.listeners.get('install')?.({ waitUntil: installWait });
    await installWait.mock.calls[0][0];
    expect(state.caches.open).toHaveBeenCalledWith('ridge-web-remote-test-version');
    expect(state.caches.open).toHaveBeenCalledWith('ridge-version-test-version');
    expect((globalThis.self as any).skipWaiting).toHaveBeenCalled();
    expect(state.clients[0].postMessage).toHaveBeenCalledWith({
      type: 'CLEAR_STORAGE', version: 'test-version',
    });

    state.cacheKeys.add('old-cache');
    const activateWait = vi.fn();
    state.listeners.get('activate')?.({ waitUntil: activateWait });
    await activateWait.mock.calls[0][0];
    expect(state.caches.delete).toHaveBeenCalledWith('old-cache');
    expect((globalThis.self as any).clients.claim).toHaveBeenCalled();
  });

  it('bypasses non-GET, cross-origin, and live control-plane requests', () => {
    expect(dispatchFetch(new Request('https://remote.test/app', { method: 'POST' }))).toBeUndefined();
    expect(dispatchFetch(new Request('https://other.test/app'))).toBeUndefined();
    expect(dispatchFetch(new Request('https://remote.test/ws'))).toBeUndefined();
    expect(dispatchFetch(new Request('https://remote.test/workspace/one'))).toBeUndefined();
  });

  it('caches successful navigation and falls back to cached shell on failure', async () => {
    const fetchMock = globalThis.fetch as ReturnType<typeof vi.fn>;
    const network = new Response('fresh shell', { status: 200 });
    fetchMock.mockResolvedValueOnce(network);
    const navigation = dispatchFetch(swRequest('https://remote.test/app', 'navigate'));
    await expect(navigation).resolves.toBe(network);
    const htmlCache = state.cacheEntries.get('ridge-html-test-version');
    expect(htmlCache?.put).toHaveBeenCalled();

    const cached = new Response('cached shell');
    fetchMock.mockRejectedValueOnce(new Error('offline'));
    state.caches.match
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce(cached);
    const fallback = dispatchFetch(swRequest('https://remote.test/app?reload=1', 'navigate'));
    await expect(fallback).resolves.toBe(cached);
  });

  it('uses cache-first immutable assets and network-first fallback for other assets', async () => {
    const fetchMock = globalThis.fetch as ReturnType<typeof vi.fn>;
    const immutable = new Response('immutable');
    state.caches.match.mockResolvedValueOnce(immutable);
    await expect(dispatchFetch(swRequest('https://remote.test/_app/immutable/chunk.js'))).resolves.toBe(immutable);
    expect(fetchMock).not.toHaveBeenCalled();

    fetchMock.mockResolvedValueOnce(new Response('new chunk', { status: 200 }));
    state.caches.match.mockResolvedValueOnce(null);
    await dispatchFetch(swRequest('https://remote.test/_app/immutable/next.js'));
    const shellCache = state.cacheEntries.get('ridge-web-remote-test-version');
    expect(shellCache?.put).toHaveBeenCalled();

    const stale = new Response('stale data');
    fetchMock.mockRejectedValueOnce(new Error('offline'));
    state.caches.match.mockResolvedValueOnce(stale);
    await expect(dispatchFetch(swRequest('https://remote.test/data.json'))).resolves.toBe(stale);
  });
});
