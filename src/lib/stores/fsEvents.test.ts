import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  isTauri: vi.fn(() => true),
  listen: vi.fn(),
  activeListener: null as ((event: { payload: unknown }) => void) | null,
  unlisten: vi.fn(),
}));

vi.mock('@tauri-apps/api/core', () => ({
  isTauri: mocks.isTauri,
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: mocks.listen,
}));

const fsEvents = await import('./fsEvents');

describe('fsEvents', () => {
  beforeEach(() => {
    mocks.isTauri.mockReturnValue(true);
    mocks.activeListener = null;
    mocks.unlisten.mockReset();
    mocks.listen.mockReset();
    mocks.listen.mockImplementation(
      async (_name: string, handler: (event: { payload: unknown }) => void) => {
        mocks.activeListener = handler;
        return mocks.unlisten;
      }
    );
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('lazily subscribes, fans out payloads, isolates handler errors, and tears down', async () => {
    const first = vi.fn();
    const second = vi.fn(() => {
      throw new Error('consumer failed');
    });
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);

    const offFirst = fsEvents.onFsChange(first);
    const offSecond = fsEvents.onFsChange(second);
    await vi.waitFor(() => expect(mocks.listen).toHaveBeenCalledTimes(1));

    const payload = { root: '/repo', paths: ['/repo/a.ts'], coalesced: false };
    mocks.activeListener?.({ payload });

    expect(first).toHaveBeenCalledWith(payload);
    expect(second).toHaveBeenCalledWith(payload);
    expect(warn).toHaveBeenCalledWith('fs-changed handler threw', expect.any(Error));

    offFirst();
    expect(mocks.unlisten).not.toHaveBeenCalled();
    offSecond();
    expect(mocks.unlisten).toHaveBeenCalledTimes(1);
  });

  it('does not touch Tauri when running outside desktop host', async () => {
    mocks.isTauri.mockReturnValue(false);
    const off = fsEvents.onFsChange(vi.fn());
    await Promise.resolve();
    expect(mocks.listen).not.toHaveBeenCalled();
    off();
  });

  it('retries subscription after a failed listener registration', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    mocks.listen
      .mockRejectedValueOnce(new Error('bridge unavailable'))
      .mockImplementationOnce(
        async (_name: string, handler: (event: { payload: unknown }) => void) => {
          mocks.activeListener = handler;
          return mocks.unlisten;
        }
      );

    const off = fsEvents.onFsChange(vi.fn());
    await vi.waitFor(() => expect(warn).toHaveBeenCalledWith('failed to subscribe fs-changed', expect.any(Error)));
    fsEvents.onFsChange(vi.fn());
    await vi.waitFor(() => expect(mocks.listen).toHaveBeenCalledTimes(2));
    off();
  });

  it('normalizes paths and expires the recently-written suppression window', () => {
    vi.useFakeTimers();
    fsEvents.markRecentlyWritten('C:\\repo\\src\\main.ts');
    expect(fsEvents.isRecentlyWritten('C:/repo/src/main.ts')).toBe(true);

    vi.advanceTimersByTime(799);
    expect(fsEvents.isRecentlyWritten('C:/repo/src/main.ts')).toBe(true);
    vi.advanceTimersByTime(1);
    expect(fsEvents.isRecentlyWritten('C:/repo/src/main.ts')).toBe(false);
  });
});
