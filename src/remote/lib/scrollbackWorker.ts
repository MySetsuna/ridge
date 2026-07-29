export type ScrollbackWorkerRequest = {
  type: 'decode';
  requestId: number;
  workspaceId: string;
  paneId: string;
  startSeq: number;
  endSeq: number;
  bytes: ArrayBuffer;
};

export type ScrollbackWorkerResult = {
  type: 'decoded';
  requestId: number;
  workspaceId: string;
  paneId: string;
  startSeq: number;
  endSeq: number;
  text: string;
};

/** Pure worker-safe decoder; DOM/kernel ownership remains on the main thread. */
export function decodeScrollback(request: ScrollbackWorkerRequest): ScrollbackWorkerResult | null {
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
    const result = decodeScrollback(event.data);
    if (result) scope.postMessage(result);
  };
}

export class ScrollbackDecoder {
  private worker: Worker | null = null;
  private requestId = 0;
  private pending = new Map<number, { resolve: (bytes: Uint8Array | null) => void; paneKey: string }>();

  constructor() {
    if (typeof Worker !== 'undefined') {
      this.worker = new Worker(new URL('./scrollbackWorker.ts', import.meta.url), { type: 'module' });
      this.worker.onmessage = (event: MessageEvent<ScrollbackWorkerResult>) => {
        const pending = this.pending.get(event.data.requestId);
        if (!pending) return;
        this.pending.delete(event.data.requestId);
        if (pending.paneKey !== `${event.data.workspaceId}:${event.data.paneId}`) return pending.resolve(null);
        pending.resolve(new TextEncoder().encode(event.data.text));
      };
      this.worker.onerror = () => this.cancel();
    }
  }

  decode(pane: { workspaceId: string; paneId: string }, startSeq: number, endSeq: number, bytes: Uint8Array): Promise<Uint8Array | null> {
    const request: ScrollbackWorkerRequest = { type: 'decode', requestId: ++this.requestId, ...pane, startSeq, endSeq, bytes: bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) };
    if (!this.worker) return Promise.resolve(decodeScrollback(request) ? bytes : null);
    return new Promise((resolve) => {
      this.pending.set(request.requestId, { resolve, paneKey: `${pane.workspaceId}:${pane.paneId}` });
      this.worker!.postMessage(request, [request.bytes]);
    });
  }

  cancel(paneKey?: string): void {
    for (const [id, pending] of this.pending) {
      if (!paneKey || pending.paneKey === paneKey) {
        this.pending.delete(id);
        pending.resolve(null);
      }
    }
  }

  dispose(): void { this.cancel(); this.worker?.terminate(); this.worker = null; }
}

if (typeof document === 'undefined' && typeof self !== 'undefined') {
  installScrollbackWorker(self as unknown as WorkerScope);
}
