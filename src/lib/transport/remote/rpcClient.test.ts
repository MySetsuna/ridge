import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { RpcClient } from './rpcClient';
import {
  RpcCancelledError,
  RpcReconnectError,
  RpcRemoteError,
  RpcTimeoutError,
  type ChannelTransport,
  type ControlFrame,
  type ControlListener,
  type PaneBytesListener,
  type StateListener,
  type TransportState,
  type Unsubscribe,
} from './types';

/**
 * In-memory ChannelTransport double. Captures outbound control frames so tests
 * can assert wire shape, and lets tests drive inbound frames + state changes.
 */
class FakeTransport implements ChannelTransport {
  sent: ControlFrame[] = [];
  private controlListeners = new Set<ControlListener>();
  private stateListeners = new Set<StateListener>();
  private _state: TransportState = 'connected';

  sendControl(frame: ControlFrame): void {
    this.sent.push(frame);
  }
  onControl(cb: ControlListener): Unsubscribe {
    this.controlListeners.add(cb);
    return () => this.controlListeners.delete(cb);
  }
  sendPaneBytes(): void {}
  onPaneBytes(_cb: PaneBytesListener): Unsubscribe {
    return () => {};
  }
  connect(): void {}
  close(): void {}
  state(): TransportState {
    return this._state;
  }
  onStateChange(cb: StateListener): Unsubscribe {
    this.stateListeners.add(cb);
    return () => this.stateListeners.delete(cb);
  }

  // ── test drivers ──
  deliver(frame: ControlFrame): void {
    for (const cb of this.controlListeners) cb(frame);
  }
  setState(s: TransportState): void {
    this._state = s;
    for (const cb of this.stateListeners) cb(s);
  }
  last(): ControlFrame | undefined {
    return this.sent[this.sent.length - 1];
  }
}

describe('RpcClient.request — envelope + correlation', () => {
  let transport: FakeTransport;
  let rpc: RpcClient;

  beforeEach(() => {
    transport = new FakeTransport();
    rpc = new RpcClient(transport);
  });

  it('emits a JSON-RPC 2.0 request envelope verbatim', () => {
    void rpc.request('read_file', { path: '/a' });
    expect(transport.last()).toEqual({
      jsonrpc: '2.0',
      id: 1,
      method: 'read_file',
      params: { path: '/a' },
    });
  });

  it('resolves with `result` when a matching success response arrives', async () => {
    const p = rpc.request<string>('read_file', { path: '/a' });
    const { id } = transport.last() as { id: number };
    transport.deliver({ jsonrpc: '2.0', id, result: 'hello' });
    await expect(p).resolves.toBe('hello');
    expect(rpc.inFlight).toBe(0);
  });

  it('rejects with RpcRemoteError carrying code + data on an error response', async () => {
    const p = rpc.request('git_push', {});
    const { id } = transport.last() as { id: number };
    transport.deliver({
      jsonrpc: '2.0',
      id,
      error: { code: -32000, message: 'auth required', data: { hint: 'token' } },
    });
    await expect(p).rejects.toBeInstanceOf(RpcRemoteError);
    await p.catch((e: RpcRemoteError) => {
      expect(e.code).toBe(-32000);
      expect(e.message).toBe('auth required');
      expect(e.data).toEqual({ hint: 'token' });
    });
  });

  it('correlates concurrent requests by id (out-of-order replies)', async () => {
    const p1 = rpc.request<number>('a');
    const p2 = rpc.request<number>('b');
    const id1 = (transport.sent[0] as { id: number }).id;
    const id2 = (transport.sent[1] as { id: number }).id;
    expect(id1).not.toBe(id2);
    transport.deliver({ jsonrpc: '2.0', id: id2, result: 2 });
    transport.deliver({ jsonrpc: '2.0', id: id1, result: 1 });
    await expect(p1).resolves.toBe(1);
    await expect(p2).resolves.toBe(2);
  });

  it('ignores responses for unknown ids', async () => {
    const p = rpc.request<number>('a');
    const { id } = transport.last() as { id: number };
    transport.deliver({ jsonrpc: '2.0', id: 9999, result: 'stale' });
    expect(rpc.inFlight).toBe(1);
    transport.deliver({ jsonrpc: '2.0', id, result: 1 });
    await expect(p).resolves.toBe(1);
  });
});

describe('RpcClient.request — timeout', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('rejects with RpcTimeoutError after the configured timeout', async () => {
    const transport = new FakeTransport();
    const rpc = new RpcClient(transport, { defaultTimeoutMs: 1000 });
    const p = rpc.request('slow');
    const rejection = expect(p).rejects.toBeInstanceOf(RpcTimeoutError);
    vi.advanceTimersByTime(1000);
    await rejection;
    expect(rpc.inFlight).toBe(0);
  });

  it('honours a per-request timeout override', async () => {
    const transport = new FakeTransport();
    const rpc = new RpcClient(transport, { defaultTimeoutMs: 10_000 });
    const p = rpc.request('slow', {}, { timeoutMs: 50 });
    const rejection = expect(p).rejects.toBeInstanceOf(RpcTimeoutError);
    vi.advanceTimersByTime(50);
    await rejection;
  });

  it('does not time out once a response arrives', async () => {
    const transport = new FakeTransport();
    const rpc = new RpcClient(transport, { defaultTimeoutMs: 1000 });
    const p = rpc.request<number>('quick');
    const { id } = transport.last() as { id: number };
    transport.deliver({ jsonrpc: '2.0', id, result: 7 });
    await expect(p).resolves.toBe(7);
    vi.advanceTimersByTime(5000); // no late rejection
  });
});

describe('RpcClient.cancel — id + AbortSignal', () => {
  let transport: FakeTransport;
  let rpc: RpcClient;

  beforeEach(() => {
    transport = new FakeTransport();
    rpc = new RpcClient(transport);
  });

  it('cancel(id) rejects with RpcCancelledError and sends $/cancel', async () => {
    const p = rpc.request('search');
    const { id } = transport.last() as { id: number };
    rpc.cancel(id);
    await expect(p).rejects.toBeInstanceOf(RpcCancelledError);
    expect(transport.last()).toEqual({
      jsonrpc: '2.0',
      method: '$/cancel',
      params: { id },
    });
    expect(rpc.inFlight).toBe(0);
  });

  it('aborting the AbortSignal rejects and sends $/cancel', async () => {
    const ac = new AbortController();
    const p = rpc.request('search', {}, { signal: ac.signal });
    const { id } = transport.sent[0] as { id: number };
    ac.abort();
    await expect(p).rejects.toBeInstanceOf(RpcCancelledError);
    const cancelFrame = transport.sent.find((f) => f.method === '$/cancel');
    expect(cancelFrame).toEqual({ jsonrpc: '2.0', method: '$/cancel', params: { id } });
  });

  it('rejects immediately if the signal is already aborted (no wire send)', async () => {
    const ac = new AbortController();
    ac.abort();
    const p = rpc.request('search', {}, { signal: ac.signal });
    await expect(p).rejects.toBeInstanceOf(RpcCancelledError);
    expect(transport.sent).toHaveLength(0);
  });

  it('cancel on an unknown id is a no-op for pending but still emits $/cancel', () => {
    rpc.cancel(42);
    expect(transport.last()).toEqual({ jsonrpc: '2.0', method: '$/cancel', params: { id: 42 } });
  });
});

describe('RpcClient — reconnect rejects in-flight', () => {
  let transport: FakeTransport;
  let rpc: RpcClient;

  beforeEach(() => {
    transport = new FakeTransport();
    rpc = new RpcClient(transport);
  });

  it('rejects all in-flight requests with RpcReconnectError on connected→reconnecting', async () => {
    const p1 = rpc.request('a');
    const p2 = rpc.request('b');
    expect(rpc.inFlight).toBe(2);
    transport.setState('reconnecting');
    await expect(p1).rejects.toBeInstanceOf(RpcReconnectError);
    await expect(p2).rejects.toBeInstanceOf(RpcReconnectError);
    expect(rpc.inFlight).toBe(0);
  });

  it('rejects in-flight on connected→disconnected (socket drop)', async () => {
    const p = rpc.request('a');
    transport.setState('disconnected');
    await expect(p).rejects.toBeInstanceOf(RpcReconnectError);
  });

  it('rejects in-flight on connected→error', async () => {
    const p = rpc.request('a');
    transport.setState('error');
    await expect(p).rejects.toBeInstanceOf(RpcReconnectError);
  });

  it('does not replay rejected requests — a late response for them is ignored', async () => {
    const p = rpc.request('a');
    const { id } = transport.last() as { id: number };
    transport.setState('reconnecting');
    await expect(p).rejects.toBeInstanceOf(RpcReconnectError);
    // A stale response arriving after reconnect must not resolve anything.
    transport.deliver({ jsonrpc: '2.0', id, result: 'late' });
    expect(rpc.inFlight).toBe(0);
  });

  it('runs resync hooks on reconnecting→connected (re-subscribe + re-pull)', () => {
    const hook = vi.fn();
    rpc.onReconnected(hook);
    transport.setState('reconnecting');
    transport.setState('connecting');
    transport.setState('connected');
    expect(hook).toHaveBeenCalledTimes(1);
  });

  it('does not run resync hooks when already connected (initial state)', () => {
    const hook = vi.fn();
    rpc.onReconnected(hook);
    // Already connected at construction → a redundant connected event is a no-op.
    transport.setState('connected');
    expect(hook).not.toHaveBeenCalled();
  });
});

describe('RpcClient — notifications', () => {
  let transport: FakeTransport;
  let rpc: RpcClient;

  beforeEach(() => {
    transport = new FakeTransport();
    rpc = new RpcClient(transport);
  });

  it('notify() emits a JSON-RPC notification with no id', () => {
    rpc.notify('use-global-workspace');
    expect(transport.last()).toEqual({ jsonrpc: '2.0', method: 'use-global-workspace' });
  });

  it('notify() includes params when provided', () => {
    rpc.notify('subscribe-pane', { paneId: 'abc' });
    expect(transport.last()).toEqual({
      jsonrpc: '2.0',
      method: 'subscribe-pane',
      params: { paneId: 'abc' },
    });
  });

  it('dispatches inbound notifications to method handlers', () => {
    const handler = vi.fn();
    rpc.onNotification('fs-changed', handler);
    transport.deliver({ jsonrpc: '2.0', method: 'fs-changed', params: { path: '/x' } });
    expect(handler).toHaveBeenCalledWith({ path: '/x' });
  });

  it('does not treat a response (has id) as a notification', () => {
    const handler = vi.fn();
    rpc.onNotification('read_file', handler);
    const p = rpc.request('read_file');
    const { id } = transport.last() as { id: number };
    transport.deliver({ jsonrpc: '2.0', id, result: 'ok' });
    expect(handler).not.toHaveBeenCalled();
    return expect(p).resolves.toBe('ok');
  });

  it('unsubscribing a notification handler stops delivery', () => {
    const handler = vi.fn();
    const off = rpc.onNotification('e', handler);
    off();
    transport.deliver({ jsonrpc: '2.0', method: 'e', params: 1 });
    expect(handler).not.toHaveBeenCalled();
  });
});

describe('RpcClient.dispose', () => {
  it('rejects in-flight requests and detaches from the transport', async () => {
    const transport = new FakeTransport();
    const rpc = new RpcClient(transport);
    const p = rpc.request('a');
    rpc.dispose();
    await expect(p).rejects.toBeInstanceOf(RpcReconnectError);
    // After dispose, further transport frames are ignored.
    transport.deliver({ jsonrpc: '2.0', method: 'x', params: 1 });
  });
});
