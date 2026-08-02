import { describe, expect, it, vi } from 'vitest';
import { decodeScrollback, installScrollbackWorker, ScrollbackDecoder } from './scrollbackWorker';

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

class ErrorReplyWorker {
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: ((event: ErrorEvent) => void) | null = null;
  postMessage(message: { requestId?: number }): void {
    queueMicrotask(() => this.onmessage?.({
      data: {
        type: 'error',
        requestId: message.requestId,
        workspaceId: 'ws',
        paneId: 'p',
        message: 'decode failed',
      },
    } as MessageEvent));
  }
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

  it('settles a worker decode error immediately instead of waiting for timeout', async () => {
    await withWorker(ErrorReplyWorker as unknown as typeof Worker, async () => {
      const decoder = new ScrollbackDecoder();
      const result = decoder.decode({ workspaceId: 'ws', paneId: 'p' }, 1, 2, requestBytes());
      await expect(result).resolves.toBeNull();
      decoder.dispose();
    });
  });

  it('worker catches decoder faults and emits a request-scoped error', () => {
    const scope = {
      onmessage: null as ((event: MessageEvent) => void) | null,
      postMessage: vi.fn(),
    };
    const PreviousDecoder = globalThis.TextDecoder;
    class ThrowingDecoder {
      decode(): string { throw new Error('malformed bytes'); }
    }
    Object.defineProperty(globalThis, 'TextDecoder', {
      configurable: true,
      writable: true,
      value: ThrowingDecoder,
    });
    try {
      installScrollbackWorker(scope);
      scope.onmessage?.({
        data: {
          type: 'decode', requestId: 42, workspaceId: 'ws', paneId: 'p',
          startSeq: 1, endSeq: 2, bytes: new ArrayBuffer(1),
        },
      } as MessageEvent);
      expect(scope.postMessage).toHaveBeenCalledWith(expect.objectContaining({
        type: 'error', requestId: 42, workspaceId: 'ws', paneId: 'p',
      }));
    } finally {
      Object.defineProperty(globalThis, 'TextDecoder', {
        configurable: true,
        writable: true,
        value: PreviousDecoder,
      });
    }
  });

  it('cancels only the destroyed pane and keeps other pane work pending', async () => {
    await withWorker(SilentWorker as unknown as typeof Worker, async () => {
      const decoder = new ScrollbackDecoder();
      const destroyed = decoder.decode({ workspaceId: 'ws', paneId: 'gone' }, 1, 2, requestBytes());
      const retained = decoder.decode({ workspaceId: 'ws', paneId: 'live' }, 1, 2, requestBytes());

      decoder.cancel('ws:gone');
      await expect(destroyed).resolves.toBeNull();

      decoder.dispose();
      await expect(retained).resolves.toBeNull();
    });
  });
});
