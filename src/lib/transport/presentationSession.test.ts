import { describe, expect, it, vi } from 'vitest';
import { presentationSession } from './presentationSession';

const client = () => ({ disconnect: vi.fn() });
const deferred = () => {
  let resolve!: (value: ReturnType<typeof client>) => void;
  const promise = new Promise<ReturnType<typeof client>>(done => { resolve = done; });
  return { promise, resolve };
};

describe('presentation bootstrap lifetime', () => {
  it('disconnects an obsolete retry instead of replacing the current client', async () => {
    const first = deferred(), second = deferred();
    const connect = vi.fn().mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const uninstall = vi.fn(), install = vi.fn(() => uninstall), status = vi.fn();
    const session = presentationSession(connect, install, status);
    const a = session.start(), b = session.start(), old = client(), current = client();
    second.resolve(current); await b;
    first.resolve(old); await a;
    expect(install).toHaveBeenCalledExactlyOnceWith(current);
    expect(old.disconnect).toHaveBeenCalledOnce();
    expect(status).toHaveBeenCalledOnce();
    session.dispose(); session.dispose();
    expect(uninstall).toHaveBeenCalledOnce();
    expect(current.disconnect).toHaveBeenCalledOnce();
  });
  it('closes a pending grant after the window unmounts without installing it', async () => {
    const pending = deferred(), install = vi.fn(() => vi.fn()), status = vi.fn();
    const session = presentationSession(() => pending.promise, install, status);
    const attempt = session.start(); session.dispose();
    const late = client(); pending.resolve(late); await attempt; await session.start();
    expect(late.disconnect).toHaveBeenCalledOnce();
    expect(install).not.toHaveBeenCalled(); expect(status).not.toHaveBeenCalled();
  });
  it('detaches the previous client before retrying and reports connection failure', async () => {
    const existing = client(), uninstall = vi.fn(), status = vi.fn();
    const failure = new Error('unavailable');
    const connect = vi.fn().mockResolvedValueOnce(existing).mockRejectedValueOnce(failure);
    const session = presentationSession(connect, () => uninstall, status);
    await session.start(); await session.start();
    expect(existing.disconnect).toHaveBeenCalledOnce(); expect(uninstall).toHaveBeenCalledOnce();
    expect(status).toHaveBeenLastCalledWith(failure);
  });
});
