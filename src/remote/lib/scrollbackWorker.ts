export type ScrollbackWorkerRequest = {
  type: 'decode';
  requestId: number;
  workspaceId: string;
  paneId: string;
  startSeq: number;
  endSeq: number;
  bytes: ArrayBuffer;
};

export type ScrollbackWorkerDecoded = {
  type: 'decoded';
  requestId: number;
  workspaceId: string;
  paneId: string;
  startSeq: number;
  endSeq: number;
  text: string;
};

export type ScrollbackWorkerResult = ScrollbackWorkerDecoded | {
  /** A malformed/failed decode is terminal for this request only. */
  type: 'error';
  requestId: number;
  workspaceId: string;
  paneId: string;
  message: string;
};

/** Pure worker-safe decoder; DOM/kernel ownership remains on the main thread. */
export function decodeScrollback(request: ScrollbackWorkerRequest): ScrollbackWorkerDecoded | null {
  if (request.startSeq >= request.endSeq || !request.workspaceId || !request.paneId) return null;
  const text = new TextDecoder().decode(request.bytes);
  return { type: 'decoded', requestId: request.requestId, workspaceId: request.workspaceId,
    paneId: request.paneId, startSeq: request.startSeq, endSeq: request.endSeq, text };
}

type WorkerScope = {
  onmessage: ((event: MessageEvent<ScrollbackWorkerRequest>) => void) | null;
  postMessage(value: ScrollbackWorkerResult): void;
};

export function installScrollbackWorker(scope: WorkerScope): void {
  scope.onmessage = (event: MessageEvent<ScrollbackWorkerRequest>) => {
    try {
      const result = decodeScrollback(event.data);
      if (result) scope.postMessage(result);
    } catch (error) {
      // Malformed/oversized input must fail this request, not tear down the
      // worker or leave the main-thread promise waiting for its timeout.
      const request = event.data;
      scope.postMessage({
        type: 'error',
        requestId: Number(request?.requestId) || 0,
        workspaceId: typeof request?.workspaceId === 'string' ? request.workspaceId : '',
        paneId: typeof request?.paneId === 'string' ? request.paneId : '',
        message: error instanceof Error ? error.message : String(error),
      });
    }
  };
}

const DECODE_TIMEOUT_MS = 10_000;
const MAX_PENDING_DECODES = 8;

type PendingDecode = {
  resolve: (bytes: Uint8Array | null) => void;
  paneKey: string;
  timer: ReturnType<typeof setTimeout>;
};

export class ScrollbackDecoder {
  private worker: Worker | null = null;
  private requestId = 0;
  private pending = new Map<number, PendingDecode>();
  private disposed = false;

  constructor() {
    if (typeof Worker !== 'undefined') {
      try {
        this.worker = new Worker(new URL('./scrollbackWorker.ts', import.meta.url), { type: 'module' });
        this.worker.onmessage = (event: MessageEvent<ScrollbackWorkerResult>) => {
          const pending = this.pending.get(event.data?.requestId);
          if (!pending) return;
          if (event.data?.type === 'error') {
            this.settle(event.data.requestId, null);
            return;
          }
          const paneKey = `${event.data.workspaceId}:${event.data.paneId}`;
          if (pending.paneKey !== paneKey || typeof event.data.text !== 'string') {
            this.settle(event.data.requestId, null);
            return;
          }
          this.settle(event.data.requestId, new TextEncoder().encode(event.data.text));
        };
        this.worker.onerror = () => this.failWorker();
      } catch {
        // CSP/bundler/older WebView can reject Worker construction. New
        // requests use the bounded synchronous decoder instead of hanging.
        this.worker = null;
      }
    }
  }

  decode(pane: { workspaceId: string; paneId: string }, startSeq: number, endSeq: number, bytes: Uint8Array): Promise<Uint8Array | null> {
    const request: ScrollbackWorkerRequest = { type: 'decode', requestId: ++this.requestId, ...pane, startSeq, endSeq, bytes: bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) };
    if (this.disposed || !this.worker || this.pending.size >= MAX_PENDING_DECODES) {
      return Promise.resolve(!this.disposed && decodeScrollback(request) ? bytes : null);
    }
    return new Promise((resolve) => {
      const id = request.requestId;
      const timer = setTimeout(() => this.settle(id, null), DECODE_TIMEOUT_MS);
      this.pending.set(id, { resolve, paneKey: `${pane.workspaceId}:${pane.paneId}`, timer });
      try {
        this.worker!.postMessage(request, [request.bytes]);
      } catch {
        this.settle(id, null);
        this.failWorker();
      }
    });
  }

  cancel(paneKey?: string): void {
    for (const [id, pending] of this.pending) {
      if (!paneKey || pending.paneKey === paneKey) {
        this.settle(id, null);
      }
    }
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.cancel();
    const worker = this.worker;
    this.worker = null;
    if (worker) {
      worker.onmessage = null;
      worker.onerror = null;
      try { worker.terminate(); } catch { /* already gone */ }
    }
  }

  private settle(id: number, bytes: Uint8Array | null): void {
    const pending = this.pending.get(id);
    if (!pending) return;
    this.pending.delete(id);
    clearTimeout(pending.timer);
    pending.resolve(bytes);
  }

  private failWorker(): void {
    const worker = this.worker;
    this.worker = null;
    if (worker) {
      worker.onmessage = null;
      worker.onerror = null;
      try { worker.terminate(); } catch { /* already gone */ }
    }
    this.cancel();
  }
}

if (typeof document === 'undefined' && typeof self !== 'undefined') {
  installScrollbackWorker(self as unknown as WorkerScope);
}
