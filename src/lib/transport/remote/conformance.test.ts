// src/lib/transport/remote/conformance.test.ts
//
// §S7 protocol-conformance suite — LAN-WS arm (handoff plan §6 S7, contract
// §7.0/§7.3/§7.4). This is the cross-cutting "防静默漂移" investment: it wires
// the REAL L2 `RpcClient` on top of the REAL L1 `LanWsAdapter`, against a fake
// host that emulates the S3 JSON-RPC-native LAN host (`server.rs`). It asserts
// the end-to-end behaviour the same suite will later run against the
// cloud-WebRTC arm, so the two transports cannot drift (decision D6):
//
//   • JSON-RPC 2.0 request/response round-trip through the stack.
//   • D9 `$/hello` handshake + capability negotiation (+ `$/bye` rejection).
//   • `$/cancel` over the wire.
//   • Full error `{code,message,data}` passthrough (the D-GM-2 fix): a
//     structured host error reaches the caller as `RpcRemoteError` with code +
//     data intact — never collapsed to a bare message.
//
// The fake host below mirrors the host's two behaviours that matter to the
// client: (1) it speaks JSON-RPC natively once it sees the client's `$/hello`,
// and (2) it round-trips ids and structured errors verbatim.

import { describe, it, expect, beforeEach, vi } from 'vitest';
import { LanWsAdapter } from './lanWsAdapter';
import { RpcClient, CLIENT_CAPABILITIES } from './rpcClient';
import type { ConnectionState, RemoteConnection } from '../../../remote/lib/wsRemote';
import { RpcRemoteError } from './types';

/**
 * Fake host that plays the S3 LAN host's JSON-RPC leg. It receives the wire
 * objects the adapter sends (already legacy-or-native), and pushes back frames
 * the adapter delivers inbound. A small programmable handler table lets each
 * test decide how the host answers a given method.
 */
class FakeJsonRpcHost {
  /** Frames the client sent to the host (post-adapter-translation). */
  received: Record<string, unknown>[] = [];
  /** Inbound delivery sink, wired to the adapter's RemoteConnection.onMessage. */
  private inbound: ((m: unknown) => void) | null = null;
  private rawSink: ((paneId: string, bytes: Uint8Array) => void) | null = null;
  private stateCb: ((s: ConnectionState) => void) | null = null;
  private _state: ConnectionState = 'connected';
  disconnected = false;
  /** Host capabilities advertised in its `$/hello` reply. */
  hostCapabilities: string[] = ['pane', 'invoke', 'fs', 'git', 'search', 'workspace', 'theme'];
  /** Host protocol version; set to 0 to force a `$/bye` mismatch. */
  hostProtocolVersion = 1;

  // ── RemoteConnection surface the adapter uses ──
  send(msg: Record<string, unknown>): void {
    this.received.push(msg);
    this.routeFromClient(msg);
  }
  onMessage(fn: (m: unknown) => void) {
    this.inbound = fn;
    return () => {
      this.inbound = null;
    };
  }
  onRawBytes(fn: (paneId: string, bytes: Uint8Array) => void) {
    this.rawSink = fn;
    return () => {
      this.rawSink = null;
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

  // ── host-side behaviour ──
  /** Per-method responder: returns the JSON-RPC `result` or throws a structured
   *  error object the host should send back. Tests override entries. */
  responders: Record<string, (params: unknown) => unknown> = {};

  private routeFromClient(msg: Record<string, unknown>): void {
    // The host only understands native JSON-RPC frames on its JSON-RPC leg.
    if (msg.jsonrpc !== '2.0') return; // legacy frame — not exercised here
    const method = msg.method as string | undefined;
    const id = msg.id;

    // D9 handshake: reply with the host's $/hello (or $/bye on mismatch).
    if (method === '$/hello') {
      if (this.hostProtocolVersion < 1) {
        this.deliver({ jsonrpc: '2.0', method: '$/bye', params: { reason: 'protocol-version-mismatch' } });
        return;
      }
      this.deliver({
        jsonrpc: '2.0',
        method: '$/hello',
        params: { protocolVersion: this.hostProtocolVersion, capabilities: this.hostCapabilities },
      });
      return;
    }
    // $/cancel is a notification; record only (no reply), like the real host.
    if (method === '$/cancel') return;
    // A request (has id) → run the responder and reply result/error.
    if (id !== undefined && id !== null && typeof method === 'string') {
      const responder = this.responders[method];
      if (!responder) return; // no reply → exercises client timeout/cancel paths
      try {
        const result = responder(msg.params);
        this.deliver({ jsonrpc: '2.0', id, result });
      } catch (e) {
        this.deliver({ jsonrpc: '2.0', id, error: e });
      }
    }
  }

  // ── test drivers ──
  deliver(frame: unknown): void {
    this.inbound?.(frame);
  }
  deliverRaw(paneId: string, bytes: Uint8Array): void {
    this.rawSink?.(paneId, bytes);
  }
  setState(s: ConnectionState): void {
    this._state = s;
    this.stateCb?.(s);
  }
}

function wire(host: FakeJsonRpcHost): { adapter: LanWsAdapter; rpc: RpcClient } {
  const adapter = new LanWsAdapter(host as unknown as RemoteConnection);
  const rpc = new RpcClient(adapter);
  return { adapter, rpc };
}

describe('S7 conformance (LAN-WS) — D9 $/hello handshake', () => {
  let host: FakeJsonRpcHost;
  let rpc: RpcClient;

  beforeEach(() => {
    host = new FakeJsonRpcHost();
    ({ rpc } = wire(host));
  });

  it('sends the client $/hello with version + capabilities', () => {
    rpc.hello();
    const hello = host.received.find((m) => m.method === '$/hello');
    expect(hello).toEqual({
      jsonrpc: '2.0',
      method: '$/hello',
      params: { protocolVersion: 1, capabilities: [...CLIENT_CAPABILITIES] },
    });
  });

  it('stores the negotiated capability intersection from the host reply', () => {
    host.hostCapabilities = ['pane', 'invoke', 'fs']; // host serves a subset
    rpc.hello();
    expect(rpc.protocol?.protocolVersion).toBe(1);
    expect(rpc.protocol?.rejected).toBe(false);
    expect(rpc.hasCapability('fs')).toBe(true);
    expect(rpc.hasCapability('git')).toBe(false); // not advertised by host
  });

  it('notifies onNegotiated subscribers with the result', () => {
    let seen: { capabilities: Set<string> } | null = null;
    rpc.onNegotiated((p) => {
      seen = p;
    });
    rpc.hello();
    expect(seen).not.toBeNull();
    expect(seen!.capabilities.has('theme')).toBe(true);
  });

  it('marks the protocol rejected on a $/bye version mismatch', () => {
    host.hostProtocolVersion = 0; // host forces $/bye
    rpc.hello();
    expect(rpc.protocol?.rejected).toBe(true);
    expect(rpc.protocol?.reason).toBe('protocol-version-mismatch');
    expect(rpc.hasCapability('fs')).toBe(false); // rejected → no panels
  });

  it('does not surface $/hello as a regular notification', () => {
    let fired = false;
    rpc.onNotification('$/hello', () => {
      fired = true;
    });
    rpc.hello();
    expect(fired).toBe(false);
  });

  it('does not double-send $/hello on the happy path (idempotent per connection)', () => {
    rpc.hello();
    rpc.hello(); // second call is a no-op until a reconnect
    const hellos = host.received.filter((m) => m.method === '$/hello');
    expect(hellos).toHaveLength(1);
  });

  it('re-handshakes after a reconnect (SPA may have updated independently)', () => {
    rpc.hello();
    expect(host.received.filter((m) => m.method === '$/hello')).toHaveLength(1);
    // Drop + reconnect → the client must greet again.
    host.setState('disconnected');
    host.setState('connecting');
    host.setState('connected');
    expect(host.received.filter((m) => m.method === '$/hello')).toHaveLength(2);
  });
});

describe('S7 conformance (LAN-WS) — JSON-RPC invoke round-trip', () => {
  let host: FakeJsonRpcHost;
  let rpc: RpcClient;

  beforeEach(() => {
    host = new FakeJsonRpcHost();
    ({ rpc } = wire(host));
    rpc.hello(); // upgrade the adapter to native JSON-RPC
  });

  it('round-trips a successful request natively after the handshake', async () => {
    host.responders.read_file = (params) => ({ echoed: params });
    const out = await rpc.request<{ echoed: unknown }>('read_file', { path: '/a' });
    expect(out).toEqual({ echoed: { path: '/a' } });
    // The request went out as a NATIVE JSON-RPC frame (not legacy invoke-request).
    const req = host.received.find((m) => m.method === 'read_file');
    expect(req).toMatchObject({ jsonrpc: '2.0', method: 'read_file', params: { path: '/a' } });
    expect(req).not.toHaveProperty('type'); // not the legacy {type:'invoke-request'} shape
  });

  it('propagates a structured error with full code + data (D-GM-2 fix)', async () => {
    host.responders.set_remote_enabled = () => {
      // The S3 host's CoreError::CapabilityDenied → to_json_rpc().
      throw { code: 1001, message: 'command not available remotely: set_remote_enabled', data: { kind: 'capability_denied' } };
    };
    const p = rpc.request('set_remote_enabled', {});
    await expect(p).rejects.toBeInstanceOf(RpcRemoteError);
    await p.catch((e: RpcRemoteError) => {
      expect(e.code).toBe(1001); // NOT collapsed to -32603 INTERNAL_ERROR
      expect(e.data).toEqual({ kind: 'capability_denied' });
      expect(e.message).toContain('set_remote_enabled');
    });
  });

  it('preserves read-only / path-traversal structured codes end to end', async () => {
    host.responders.write_file = () => {
      throw { code: 1002, message: 'remote filesystem is read-only', data: { kind: 'read_only' } };
    };
    await rpc.request('write_file', { path: '/a', content: 'x' }).catch((e: RpcRemoteError) => {
      expect(e.code).toBe(1002);
      expect(e.data).toEqual({ kind: 'read_only' });
    });
  });

  it('correlates concurrent requests by id through the adapter', async () => {
    host.responders.a = () => 1;
    host.responders.b = () => 2;
    const [a, b] = await Promise.all([rpc.request<number>('a'), rpc.request<number>('b')]);
    expect(a).toBe(1);
    expect(b).toBe(2);
  });
});

describe('S7 conformance (LAN-WS) — pre-handshake legacy fallback', () => {
  it('sends invoke as the LEGACY envelope before the host $/hello reply', () => {
    const host = new FakeJsonRpcHost();
    const { rpc } = wire(host);
    // No handshake yet → adapter still in legacy-translation mode.
    void rpc.request('read_file', { path: '/a' });
    const legacy = host.received.find((m) => m.type === 'invoke-request');
    expect(legacy).toEqual({ type: 'invoke-request', cmd: 'read_file', args: { path: '/a' }, _reqId: 1 });
  });

  it('a legacy invoke-result still resolves the request (old-host compatibility)', async () => {
    const host = new FakeJsonRpcHost();
    const { rpc } = wire(host);
    const p = rpc.request<string>('read_file', { path: '/a' });
    const reqId = (host.received.find((m) => m.type === 'invoke-request') as { _reqId: number })._reqId;
    // Old host replies in the legacy envelope; the adapter maps it to JSON-RPC.
    host.deliver({ type: 'invoke-result', _reqId: reqId, _result: 'hello' });
    await expect(p).resolves.toBe('hello');
  });
});

describe('S7 conformance (LAN-WS) — $/cancel over the wire', () => {
  let host: FakeJsonRpcHost;
  let rpc: RpcClient;

  beforeEach(() => {
    host = new FakeJsonRpcHost();
    ({ rpc } = wire(host));
    rpc.hello();
  });

  it('cancel(id) emits a native $/cancel frame and rejects the request', async () => {
    // Host never responds → request stays in-flight until cancelled.
    const p = rpc.request('text_search', { query: 'x' });
    const id = (host.received.find((m) => m.method === 'text_search') as { id: number }).id;
    rpc.cancel(id);
    await expect(p).rejects.toThrow(/cancelled/);
    const cancel = host.received.find((m) => m.method === '$/cancel');
    expect(cancel).toEqual({ jsonrpc: '2.0', method: '$/cancel', params: { id } });
  });

  it('AbortSignal cancellation reaches the host as $/cancel', async () => {
    const ac = new AbortController();
    const p = rpc.request('text_search', { query: 'x' }, { signal: ac.signal });
    const id = (host.received.find((m) => m.method === 'text_search') as { id: number }).id;
    ac.abort();
    await expect(p).rejects.toThrow(/cancelled/);
    const cancel = host.received.find((m) => m.method === '$/cancel');
    expect(cancel).toEqual({ jsonrpc: '2.0', method: '$/cancel', params: { id } });
  });
});

describe('S7 conformance (LAN-WS) — pane bytes + notifications', () => {
  it('forwards raw pane bytes through the adapter to the RPC consumer', () => {
    const host = new FakeJsonRpcHost();
    const adapter = new LanWsAdapter(host as unknown as RemoteConnection);
    const got: { paneId: string; bytes: Uint8Array }[] = [];
    adapter.onPaneBytes((paneId, bytes) => got.push({ paneId, bytes }));
    const bytes = new Uint8Array([0x1b, 0x5b, 0x41]);
    host.deliverRaw('pane-7', bytes);
    expect(got).toEqual([{ paneId: 'pane-7', bytes }]);
  });

  it('host event pushes reach onNotification consumers (post-handshake)', () => {
    const host = new FakeJsonRpcHost();
    const { rpc } = wire(host);
    rpc.hello();
    const handler = vi.fn();
    rpc.onNotification('fs-changed', handler);
    host.deliver({ jsonrpc: '2.0', method: 'fs-changed', params: { path: '/x' } });
    expect(handler).toHaveBeenCalledWith({ path: '/x' });
  });
});
