import { describe, expect, it } from 'vitest';
import { decodeScrollback, ScrollbackDecoder } from './scrollbackWorker';

const requestBytes = () => new Uint8Array([111, 107]);

function withWorker<T>(worker: typeof Worker, run: () => Promise<T>): Promise<T> {
  const previous = (globalThis as typeof globalThis & { Worker?: typeof Worker }).Worker;
  Object.defineProperty(globalThis, 'Worker', { configurable: true, writable: true, value: worker });
  return run().finally(() => {
    if (previous) Object.defineProperty(globalThis, 'Worker', { configurable: true, writable: true, value: previous });
    else Reflect.deleteProperty(globalThis, 'Worker');
  });
}

class ClosedWorker {
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: ((event: ErrorEvent) => void) | null = null;
  postMessage(): void { throw new Error('message channel closed'); }
  terminate(): void {}
}

class SilentWorker {
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: ((event: ErrorEvent) => void) | null = null;
  postMessage(): void {}
  terminate(): void {}
}

describe('scrollback worker protocol', () => {
  it('decodes bounded page without owning pane state', () => {
    const result = decodeScrollback({ type: 'decode', requestId: 7, workspaceId: 'ws', paneId: 'p', startSeq: 1, endSeq: 2, bytes: new TextEncoder().encode('ok').buffer });
    expect(result?.text).toBe('ok');
    expect(result?.requestId).toBe(7);
  });
  it('rejects invalid or stale seq range', () => {
    expect(decodeScrollback({ type: 'decode', requestId: 1, workspaceId: 'ws', paneId: 'p', startSeq: 2, endSeq: 2, bytes: new ArrayBuffer(0) })).toBeNull();
  });

  it('settles and falls back when the worker message channel is closed', async () => {
    await withWorker(ClosedWorker as unknown as typeof Worker, async () => {
      const decoder = new ScrollbackDecoder();
      await expect(decoder.decode({ workspaceId: 'ws', paneId: 'p' }, 1, 2, requestBytes())).resolves.toBeNull();
      await expect(decoder.decode({ workspaceId: 'ws', paneId: 'p' }, 1, 2, requestBytes())).resolves.toEqual(requestBytes());
      decoder.dispose();
    });
  });

  it('resolves pending work on dispose and leaves no worker callback alive', async () => {
    await withWorker(SilentWorker as unknown as typeof Worker, async () => {
      const decoder = new ScrollbackDecoder();
      const result = decoder.decode({ workspaceId: 'ws', paneId: 'p' }, 1, 2, requestBytes());
      decoder.dispose();
      await expect(result).resolves.toBeNull();
      decoder.dispose();
    });
  });
});
