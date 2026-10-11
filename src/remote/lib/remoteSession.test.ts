import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { clearRemoteToken, readRemoteToken, remoteSessionValidity, writeRemoteToken } from './remoteSession';

function storage() {
  const values = new Map<string, string>();
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => { values.set(key, value); },
    removeItem: (key: string) => { values.delete(key); },
  };
}

describe('retained Remote session', () => {
  beforeEach(() => {
    vi.stubGlobal('localStorage', storage());
    vi.stubGlobal('sessionStorage', storage());
  });
  afterEach(() => { vi.unstubAllGlobals(); });

  it('reuses the saved token after reload and clears both stores on invalidation', () => {
    sessionStorage.setItem('ridge_remote_token', 'old');
    writeRemoteToken('current');
    expect(readRemoteToken()).toBe('current');
    expect(sessionStorage.getItem('ridge_remote_token')).toBeNull();
    clearRemoteToken();
    expect(readRemoteToken()).toBeNull();
  });

  it('retains a tab session when persistent storage is blocked', () => {
    vi.stubGlobal('localStorage', {
      getItem: () => { throw new Error('blocked'); },
      setItem: () => { throw new Error('blocked'); },
      removeItem: () => { throw new Error('blocked'); },
    });
    writeRemoteToken('tab-session');
    expect(readRemoteToken()).toBe('tab-session');
    clearRemoteToken();
    expect(readRemoteToken()).toBeNull();
  });

  it('uses the new tab token if quota prevents replacing an old persistent token', () => {
    vi.stubGlobal('localStorage', {
      getItem: () => 'old',
      setItem: () => { throw new Error('quota'); },
      removeItem: () => {},
    });
    writeRemoteToken('new');
    expect(readRemoteToken()).toBe('new');
  });

  it('distinguishes authoritative expiry from unavailable or malformed checks', async () => {
    const request = vi.fn().mockRejectedValueOnce(new Error('offline'))
      .mockResolvedValueOnce({ ok: false })
      .mockResolvedValueOnce({ ok: true, json: async () => ({}) })
      .mockResolvedValueOnce({ ok: true, json: async () => ({ valid: true }) })
      .mockResolvedValueOnce({ ok: true, json: async () => ({ valid: false }) });
    vi.stubGlobal('fetch', request);
    writeRemoteToken('retained');
    const signal = new AbortController().signal;
    for (const expected of [null, null, null, true, false]) {
      expect(await remoteSessionValidity('retained', signal)).toBe(expected);
      expect(readRemoteToken()).toBe('retained');
    }
    expect(request).toHaveBeenCalledWith('/session?token=retained', { signal, cache: 'no-store' });
  });
});
