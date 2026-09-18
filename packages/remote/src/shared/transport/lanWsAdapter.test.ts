import { describe, it, expect, beforeEach, vi } from 'vitest';
import { LanWsAdapter } from './lanWsAdapter';
import type { ConnectionState } from './wsRemote';
import { RemoteConnection } from './wsRemote';
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
  setState(s: ConnectionState): void {
    this._state = s;
    this.stateCb?.(s);
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

  it('fans state changes out to multiple independent subscribers', () => {
    // Regression: the adapter owns a listener Set (one upstream subscription), so
    // two consumers (e.g. L2 RpcClient + the UI) both see transitions instead of
    // the second clobbering the first's upstream slot.
    const { conn, adapter } = makeAdapter();
    const a: string[] = [];
    const b: string[] = [];
    adapter.onStateChange((s) => a.push(s));
    adapter.onStateChange((s) => b.push(s));
    conn.setState('disconnected');
    expect(a).toContain('disconnected');
    expect(b).toContain('disconnected');
  });
});

describe('LanWsAdapter — authState (FIX-4)', () => {
  it('reports authorized while connected (token verified at WS upgrade)', () => {
    const { adapter } = makeAdapter(); // fake starts `connected`
    expect(adapter.authState()).toBe('authorized');
  });

  it('reports pending when not yet connected', () => {
    const conn = new FakeRemoteConnection();
    conn.setState('connecting');
    const adapter = new LanWsAdapter(conn as unknown as RemoteConnection);
    expect(adapter.authState()).toBe('pending');
  });

  it('emits authorized/pending on the connect edge, deduped', () => {
    const conn = new FakeRemoteConnection();
    conn.setState('connecting');
    const adapter = new LanWsAdapter(conn as unknown as RemoteConnection);
    const seen: string[] = [];
    adapter.onAuthChange((auth) => seen.push(auth));
    conn.setState('connected'); // → authorized
    conn.setState('connected'); // same → deduped (no emit)
    conn.setState('disconnected'); // → pending
    expect(seen).toEqual(['authorized', 'pending']);
  });
});

describe('LanWsAdapter — v9-16 subscribe-pane registration parity (CHG-032)', () => {
  // Desktop Web Remote output broke because `bridge.subscribePane` only sent a
  // `subscribe-pane` notification without establishing the pane registration
  // (`paneKeysById`) that binary PTY dispatch requires. The adapter must route
  // the notification through `RemoteConnection.subscribePane` — the exact
  // function the mobile path uses. These tests drive a REAL RemoteConnection
  // (no socket connect needed: registration + dispatch are socket-independent)
  // with host-format `pane_frame` bytes (16B UUID prefix + raw PTY payload).
  const WS_ID = '11111111-2222-4333-8444-555555555555';
  const PANE_A = 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee';
  const PANE_B = 'ffffffff-1111-4222-8333-444444444444';
  const MARK_A = 'V916_DESKTOP_MARKER_A_Q7Z3';
  const MARK_B = 'V916_DESKTOP_MARKER_B_K9W2';

  function uuidToBytes(uuid: string): Uint8Array {
    const hex = uuid.replaceAll('-', '');
    const out = new Uint8Array(16);
    for (let i = 0; i < 16; i++) out[i] = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16);
    return out;
  }

  function hostPaneBinary(paneId: string, text: string): ArrayBuffer {
    const prefix = uuidToBytes(paneId);
    const body = new TextEncoder().encode(text);
    const frame = new Uint8Array(16 + body.length);
    frame.set(prefix, 0);
    frame.set(body, 16);
    return frame.buffer.slice(0);
  }

  function makeLiveAdapter(): {
    conn: RemoteConnection;
    adapter: LanWsAdapter;
    sent: Record<string, unknown>[];
  } {
    const conn = new RemoteConnection();
    const sent: Record<string, unknown>[] = [];
    vi.spyOn(conn, 'send').mockImplementation((msg) => {
      sent.push(msg as Record<string, unknown>);
    });
    return { conn, adapter: new LanWsAdapter(conn), sent };
  }

  function injectBinary(conn: RemoteConnection, buf: ArrayBuffer): void {
    // Calls the real private dispatch with host-format bytes (the socket event
    // wrapper only does `new Uint8Array(data)` + the 0x13 tag check, both
    // covered by wsRemote.behavior tests over a real fake socket).
    (conn as unknown as { _handleBinaryMessage: (data: ArrayBuffer) => void })._handleBinaryMessage(buf);
  }

  it('routes subscribe-pane through conn.subscribePane: exactly one legacy wire frame', () => {
    const { adapter, sent } = makeLiveAdapter();
    adapter.sendControl({
      jsonrpc: '2.0',
      method: 'subscribe-pane',
      params: { paneId: PANE_A, workspaceId: WS_ID, active: true },
    });
    // Registration + a single send inside conn.subscribePane — no double
    // subscribe, and the wire shape is the legacy frame the host dispatches.
    expect(sent).toEqual([{ type: 'subscribe-pane', paneId: PANE_A, workspaceId: WS_ID, active: true }]);
  });

  it('delivers binary pane_frame to onPaneBytes after subscribe (desktop output parity)', () => {
    const { conn, adapter } = makeLiveAdapter();
    const got: { paneId: string; text: string }[] = [];
    adapter.onPaneBytes((paneId, bytes) => got.push({ paneId, text: new TextDecoder().decode(bytes) }));
    adapter.sendControl({
      jsonrpc: '2.0',
      method: 'subscribe-pane',
      params: { paneId: PANE_A, workspaceId: WS_ID, active: true },
    });
    injectBinary(conn, hostPaneBinary(PANE_A, `echo ${MARK_A}\r\n${MARK_A}\r\n`));
    expect(got.length).toBe(1);
    expect(got[0].paneId).toBe(PANE_A);
    expect(got[0].text).toContain(MARK_A);
  });

  it('drops binary for panes that were never subscribed (registration guard intact)', () => {
    const { conn, adapter } = makeLiveAdapter();
    const got: unknown[] = [];
    adapter.onPaneBytes((paneId, bytes) => got.push([paneId, bytes]));
    injectBinary(conn, hostPaneBinary(PANE_A, MARK_A));
    expect(got).toEqual([]);
  });

  it('routes rapid A→B→A frames to their own panes with zero cross-leak', () => {
    const { conn, adapter } = makeLiveAdapter();
    const got: { paneId: string; text: string }[] = [];
    adapter.onPaneBytes((paneId, bytes) => got.push({ paneId, text: new TextDecoder().decode(bytes) }));
    for (const paneId of [PANE_A, PANE_B]) {
      adapter.sendControl({ jsonrpc: '2.0', method: 'subscribe-pane', params: { paneId, workspaceId: WS_ID } });
    }
    injectBinary(conn, hostPaneBinary(PANE_B, MARK_B));
    injectBinary(conn, hostPaneBinary(PANE_A, MARK_A));
    expect(got.map((g) => g.paneId)).toEqual([PANE_B, PANE_A]);
    expect(got[0].text).toContain(MARK_B);
    expect(got[0].text).not.toContain(MARK_A);
    expect(got[1].text).toContain(MARK_A);
    expect(got[1].text).not.toContain(MARK_B);
  });

  it('unsubscribe-pane unregisters locally AND still forwards exactly one frame', () => {
    const { conn, adapter } = makeLiveAdapter();
    const got: unknown[] = [];
    adapter.onPaneBytes((paneId, bytes) => got.push([paneId, bytes]));
    adapter.sendControl({ jsonrpc: '2.0', method: 'subscribe-pane', params: { paneId: PANE_A, workspaceId: WS_ID } });
    injectBinary(conn, hostPaneBinary(PANE_A, MARK_A));
    expect(got.length).toBe(1);
    adapter.sendControl({ jsonrpc: '2.0', method: 'unsubscribe-pane', params: { paneId: PANE_A, workspaceId: WS_ID } });
    injectBinary(conn, hostPaneBinary(PANE_A, MARK_A));
    expect(got.length).toBe(1); // stale post-unsubscribe frame is dropped
  });

  it('forwards the unsubscribe frame on the wire exactly once', () => {
    const { adapter, sent } = makeLiveAdapter();
    adapter.sendControl({ jsonrpc: '2.0', method: 'unsubscribe-pane', params: { paneId: PANE_A, workspaceId: WS_ID } });
    expect(sent).toEqual([{ type: 'unsubscribe-pane', paneId: PANE_A, workspaceId: WS_ID }]);
  });

  it('re-subscribe after unregister restores delivery (reconnect analogue)', () => {
    const { conn, adapter } = makeLiveAdapter();
    const got: unknown[] = [];
    adapter.onPaneBytes((paneId, bytes) => got.push([paneId, bytes]));
    const sub = { jsonrpc: '2.0', method: 'subscribe-pane', params: { paneId: PANE_A, workspaceId: WS_ID } };
    adapter.sendControl(sub);
    adapter.sendControl({ jsonrpc: '2.0', method: 'unsubscribe-pane', params: { paneId: PANE_A, workspaceId: WS_ID } });
    adapter.sendControl(sub); // reconnect resync re-notifies → re-registers
    injectBinary(conn, hostPaneBinary(PANE_A, MARK_A));
    expect(got.length).toBe(1);
  });

  it('malformed subscribe (missing ids) falls back to the legacy bare send', () => {
    const { adapter, sent } = makeLiveAdapter();
    adapter.sendControl({ jsonrpc: '2.0', method: 'subscribe-pane', params: { paneId: 'abc' } });
    expect(sent).toEqual([{ type: 'subscribe-pane', paneId: 'abc' }]);
  });

  it('never intercepts a request-shaped subscribe-pane (with id): no response hang', () => {
    const { conn, adapter, sent } = makeLiveAdapter();
    const got: unknown[] = [];
    adapter.onPaneBytes((paneId, bytes) => got.push([paneId, bytes]));
    adapter.sendControl({
      jsonrpc: '2.0',
      id: 3,
      method: 'subscribe-pane',
      params: { paneId: PANE_A, workspaceId: WS_ID },
    });
    // Translated as a normal invoke-request (the host dispatches it and
    // replies); crucially NO local registration happens through the
    // response-less subscribePane path, so the RPC cannot hang.
    expect(sent).toEqual([{
      type: 'invoke-request',
      cmd: 'subscribe-pane',
      args: { paneId: PANE_A, workspaceId: WS_ID },
      _reqId: 3,
    }]);
    injectBinary(conn, hostPaneBinary(PANE_A, MARK_A));
    expect(got).toEqual([]);
  });
});
