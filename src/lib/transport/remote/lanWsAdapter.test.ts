import { describe, it, expect, beforeEach } from 'vitest';
import { LanWsAdapter } from './lanWsAdapter';
import type { ConnectionState } from '../../../remote/lib/wsRemote';
import type { RemoteConnection } from '../../../remote/lib/wsRemote';
import type { ControlFrame } from './types';

/**
 * Minimal structural stub of RemoteConnection — only the surface the adapter
 * touches. Lets us assert the adapter translates JSON-RPC ↔ the legacy LAN wire
 * format the host (server.rs) speaks, i.e. "behavior unchanged".
 */
class FakeRemoteConnection {
  sent: Record<string, unknown>[] = [];
  private messageCb: ((m: unknown) => void) | null = null;
  private rawCb: ((paneId: string, bytes: Uint8Array) => void) | null = null;
  private stateCb: ((s: ConnectionState) => void) | null = null;
  private _state: ConnectionState = 'connected';
  disconnected = false;

  send(msg: Record<string, unknown>): void {
    this.sent.push(msg);
  }
  onMessage(fn: (m: unknown) => void) {
    this.messageCb = fn;
    return () => {
      this.messageCb = null;
    };
  }
  onRawBytes(fn: (paneId: string, bytes: Uint8Array) => void) {
    this.rawCb = fn;
    return () => {
      this.rawCb = null;
    };
  }
  onStateChange(fn: (s: ConnectionState) => void) {
    this.stateCb = fn;
    return () => {
      this.stateCb = null;
    };
  }
  state(): ConnectionState {
    return this._state;
  }
  disconnect(): void {
    this.disconnected = true;
  }

  // ── test drivers ──
  deliverMessage(m: unknown): void {
    this.messageCb?.(m);
  }
  deliverRaw(paneId: string, bytes: Uint8Array): void {
    this.rawCb?.(paneId, bytes);
  }
  last(): Record<string, unknown> | undefined {
    return this.sent[this.sent.length - 1];
  }
}

function makeAdapter(): { conn: FakeRemoteConnection; adapter: LanWsAdapter } {
  const conn = new FakeRemoteConnection();
  const adapter = new LanWsAdapter(conn as unknown as RemoteConnection);
  return { conn, adapter };
}

describe('LanWsAdapter — outbound JSON-RPC → legacy wire', () => {
  let conn: FakeRemoteConnection;
  let adapter: LanWsAdapter;

  beforeEach(() => {
    ({ conn, adapter } = makeAdapter());
  });

  it('maps a JSON-RPC request to invoke-request (cmd/args/_reqId)', () => {
    adapter.sendControl({ jsonrpc: '2.0', id: 7, method: 'read_file', params: { path: '/a' } });
    expect(conn.last()).toEqual({
      type: 'invoke-request',
      cmd: 'read_file',
      args: { path: '/a' },
      _reqId: 7,
    });
  });

  it('maps a JSON-RPC notification to a flat legacy control frame', () => {
    adapter.sendControl({ jsonrpc: '2.0', method: 'use-global-workspace' });
    expect(conn.last()).toEqual({ type: 'use-global-workspace' });
  });

  it('spreads notification params into the legacy control frame', () => {
    adapter.sendControl({ jsonrpc: '2.0', method: 'subscribe-pane', params: { paneId: 'abc' } });
    expect(conn.last()).toEqual({ type: 'subscribe-pane', paneId: 'abc' });
  });

  it('forwards a $/cancel notification natively (host JSON-RPC leg handles it)', () => {
    // §S3: `$/`-control methods always pass through as native JSON-RPC so the
    // host's JSON-RPC leg processes them; they are NOT downgraded to a legacy
    // `{type:'cancel'}` frame (the legacy host had no such handler anyway).
    adapter.sendControl({ jsonrpc: '2.0', method: '$/cancel', params: { id: 5 } });
    expect(conn.last()).toEqual({ jsonrpc: '2.0', method: '$/cancel', params: { id: 5 } });
  });

  it('forwards a $/hello notification natively', () => {
    adapter.sendControl({
      jsonrpc: '2.0',
      method: '$/hello',
      params: { protocolVersion: 1, capabilities: ['invoke'] },
    });
    expect(conn.last()).toEqual({
      jsonrpc: '2.0',
      method: '$/hello',
      params: { protocolVersion: 1, capabilities: ['invoke'] },
    });
  });

  it('upgrades to native JSON-RPC after the host $/hello reply, so invoke errors carry code/data', () => {
    // Before negotiation: legacy translation (byte-for-byte unchanged).
    adapter.sendControl({ jsonrpc: '2.0', id: 1, method: 'read_file', params: { path: '/a' } });
    expect(conn.last()).toEqual({ type: 'invoke-request', cmd: 'read_file', args: { path: '/a' }, _reqId: 1 });
    // Host proves it speaks JSON-RPC.
    conn.deliverMessage({ jsonrpc: '2.0', method: '$/hello', params: { protocolVersion: 1, capabilities: ['invoke'] } });
    // After negotiation: native pass-through (full error fidelity).
    adapter.sendControl({ jsonrpc: '2.0', id: 2, method: 'read_file', params: { path: '/b' } });
    expect(conn.last()).toEqual({ jsonrpc: '2.0', id: 2, method: 'read_file', params: { path: '/b' } });
  });

  it('passes through an already-legacy control frame unchanged', () => {
    adapter.sendControl({ type: 'list-panes' } as ControlFrame);
    expect(conn.last()).toEqual({ type: 'list-panes' });
  });
});

describe('LanWsAdapter — inbound legacy → JSON-RPC', () => {
  let conn: FakeRemoteConnection;
  let adapter: LanWsAdapter;
  let frames: ControlFrame[];

  beforeEach(() => {
    ({ conn, adapter } = makeAdapter());
    frames = [];
    adapter.onControl((f) => frames.push(f));
  });

  it('maps invoke-result (_result) to a JSON-RPC success response', () => {
    conn.deliverMessage({ type: 'invoke-result', _reqId: 7, _result: 'hi' });
    expect(frames).toContainEqual({ jsonrpc: '2.0', id: 7, result: 'hi' });
  });

  it('maps invoke-result (_error) to a JSON-RPC error response', () => {
    conn.deliverMessage({ type: 'invoke-result', _reqId: 7, _error: 'boom' });
    const errFrame = frames.find((f) => 'error' in f) as ControlFrame & {
      error: { message: string };
    };
    expect(errFrame.id).toBe(7);
    expect(errFrame.error.message).toBe('boom');
  });

  it('maps invoke-result with null _result to result:null', () => {
    conn.deliverMessage({ type: 'invoke-result', _reqId: 1, _result: null });
    expect(frames).toContainEqual({ jsonrpc: '2.0', id: 1, result: null });
  });

  it('passes through host event pushes verbatim', () => {
    const evt = { type: 'event', name: 'fs-changed', payload: { path: '/x' } };
    conn.deliverMessage(evt);
    expect(frames).toContainEqual(evt);
  });

  it('passes a native JSON-RPC error response through with full code/data (D-GM-2 fix)', () => {
    // The S3 host's JSON-RPC leg emits a structured error; the adapter must NOT
    // re-wrap it (which would lose code/data) — it forwards verbatim to L2.
    const errResp = {
      jsonrpc: '2.0',
      id: 9,
      error: { code: 1001, message: 'command not available remotely: x', data: { kind: 'capability_denied' } },
    };
    conn.deliverMessage(errResp);
    expect(frames).toContainEqual(errResp);
  });

  it('passes a native JSON-RPC success response through verbatim', () => {
    const okResp = { jsonrpc: '2.0', id: 10, result: { ok: true } };
    conn.deliverMessage(okResp);
    expect(frames).toContainEqual(okResp);
  });
});

describe('LanWsAdapter — pane bytes + lifecycle', () => {
  it('forwards raw pane bytes to onPaneBytes listeners', () => {
    const { conn, adapter } = makeAdapter();
    const got: { paneId: string; bytes: Uint8Array }[] = [];
    adapter.onPaneBytes((paneId, bytes) => got.push({ paneId, bytes }));
    const bytes = new Uint8Array([1, 2, 3]);
    conn.deliverRaw('pane-1', bytes);
    expect(got).toEqual([{ paneId: 'pane-1', bytes }]);
  });

  it('reflects RemoteConnection state', () => {
    const { adapter } = makeAdapter();
    expect(adapter.state()).toBe('connected');
  });

  it('propagates state changes', () => {
    const { conn, adapter } = makeAdapter();
    const states: string[] = [];
    adapter.onStateChange((s) => states.push(s));
    (conn as unknown as { stateCb: (s: ConnectionState) => void }).stateCb('disconnected');
    expect(states).toContain('disconnected');
  });

  it('close() disconnects the underlying connection', () => {
    const { conn, adapter } = makeAdapter();
    adapter.close();
    expect(conn.disconnected).toBe(true);
  });
});
