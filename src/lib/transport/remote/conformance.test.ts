// src/lib/transport/remote/conformance.test.ts
//
// §S7 protocol-conformance suite (handoff plan §6 S7, contract §7.0/§7.3/§7.4).
// This is the cross-cutting "防静默漂移" investment, decision D6: the SAME suite
// runs against BOTH transports — the LAN-WS arm (`LanWsAdapter`) and the
// cloud-WebRTC arm (`CloudWebrtcAdapter`) — so the two legs cannot drift. Each
// arm wires the REAL L2 `RpcClient` on top of the REAL L1 adapter, against a
// fake host that emulates the S3 JSON-RPC-native host (`server.rs` / the cloud
// host on the far side of the DataChannel). It asserts:
//
//   • JSON-RPC 2.0 request/response round-trip through the full stack.
//   • D9 `$/hello` handshake + capability negotiation (+ `$/bye` rejection).
//   • `$/cancel` over the wire.
//   • Full error `{code,message,data}` passthrough (the D-GM-2 fix): a
//     structured host error reaches the caller as `RpcRemoteError` with code +
//     data intact — never collapsed to a bare message.
//   • Raw pane bytes + host event notifications.
//
// The LAN-only legacy fallback (pre-handshake legacy invoke envelope) stays in
// its own block — the cloud leg is JSON-RPC-native from the first frame and has
// no legacy mode, so that behaviour is transport-specific by design.

import { describe, it, expect, beforeEach, vi } from 'vitest';
import { LanWsAdapter } from './lanWsAdapter';
import { createCloudWebrtcTransportWith } from './cloudWebrtcAdapter';
import { demuxFrame, encodeJsonFrame, encodePaneFrame } from './cloudMux';
import { RpcClient, CLIENT_CAPABILITIES } from './rpcClient';
import type { ConnectionState, RemoteConnection } from '../../../remote/lib/wsRemote';
import type {
  CloudConnectionCallbacks,
  CloudConnectionState,
  RemoteConnectionProvider,
} from '../../remote/cloud/connectionProvider';
import type { ChannelTransport } from './types';
import { RpcRemoteError } from './types';

// ─────────────────────────────────────────────────────────────────────────────
// Transport-agnostic host behaviour (the bit that MUST be identical across legs)
// ─────────────────────────────────────────────────────────────────────────────

/** Mutable host state a test tweaks before driving the client. */
interface HostState {
  responders: Record<string, (params: unknown) => unknown>;
  hostCapabilities: string[];
  hostProtocolVersion: number;
}

/**
 * Compute the host's reply to one client JSON-RPC frame — the single source of
 * truth both transport fakes route through, so neither leg can answer
 * differently. Returns the frame to push back inbound, or `null` for no reply
 * (notifications, unknown methods → exercises the client's timeout/cancel path).
 */
function hostReply(msg: Record<string, unknown>, host: HostState): Record<string, unknown> | null {
  if (msg.jsonrpc !== '2.0') return null; // legacy frame — not exercised here
  const method = msg.method as string | undefined;
  const id = msg.id;

  if (method === '$/hello') {
    if (host.hostProtocolVersion < 1) {
      return { jsonrpc: '2.0', method: '$/bye', params: { reason: 'protocol-version-mismatch' } };
    }
    return {
      jsonrpc: '2.0',
      method: '$/hello',
      params: { protocolVersion: host.hostProtocolVersion, capabilities: host.hostCapabilities },
    };
  }
  if (method === '$/cancel') return null; // notification — record only
  if (id !== undefined && id !== null && typeof method === 'string') {
    const responder = host.responders[method];
    if (!responder) return null; // no reply → exercises timeout/cancel
    try {
      return { jsonrpc: '2.0', id, result: responder(msg.params) };
    } catch (e) {
      return { jsonrpc: '2.0', id, error: e };
    }
  }
  return null;
}

/** A wired conformance arm: the client + drivers, presented identically per leg. */
interface Harness {
  rpc: RpcClient;
  adapter: ChannelTransport;
  /** JSON objects the client sent (post-adapter-translation, demuxed for cloud). */
  received: Record<string, unknown>[];
  /** Mutable host state (tests set `host.responders.foo = …`, caps, version). */
  host: HostState;
  /** Push an inbound JSON control frame (host → client). */
  deliver(frame: Record<string, unknown>): void;
  /** Push inbound raw pane bytes. */
  deliverRaw(paneId: string, bytes: Uint8Array): void;
  /** Drive the transport connection state. */
  setState(s: ConnectionState): void;
}

function freshHost(): HostState {
  return {
    responders: {},
    hostCapabilities: ['pane', 'invoke', 'fs', 'git', 'search', 'workspace', 'theme'],
    hostProtocolVersion: 1,
  };
}

// ── LAN-WS arm: fake `RemoteConnection` behind `LanWsAdapter` ──
function makeLanHarness(): Harness {
  const host = freshHost();
  const received: Record<string, unknown>[] = [];
  let inbound: ((m: unknown) => void) | null = null;
  let rawSink: ((paneId: string, bytes: Uint8Array) => void) | null = null;
  let stateCb: ((s: ConnectionState) => void) | null = null;
  let curState: ConnectionState = 'connected';

  const conn = {
    send(msg: Record<string, unknown>) {
      received.push(msg);
      const reply = hostReply(msg, host);
      if (reply) inbound?.(reply);
    },
    onMessage(fn: (m: unknown) => void) {
      inbound = fn;
      return () => {
        inbound = null;
      };
    },
    onRawBytes(fn: (paneId: string, bytes: Uint8Array) => void) {
      rawSink = fn;
      return () => {
        rawSink = null;
      };
    },
    onStateChange(fn: (s: ConnectionState) => void) {
      stateCb = fn;
      return () => {
        stateCb = null;
      };
    },
    state: () => curState,
    disconnect() {},
  };

  const adapter = new LanWsAdapter(conn as unknown as RemoteConnection);
  const rpc = new RpcClient(adapter);
  return {
    rpc,
    adapter,
    received,
    host,
    deliver: (frame) => inbound?.(frame),
    deliverRaw: (paneId, bytes) => rawSink?.(paneId, bytes),
    setState: (s) => {
      curState = s;
      stateCb?.(s);
    },
  };
}

// ── cloud-WebRTC arm: fake `RemoteConnectionProvider` behind `CloudWebrtcAdapter` ──
function makeCloudHarness(): Harness {
  const host = freshHost();
  const received: Record<string, unknown>[] = [];

  // Map the conformance `ConnectionState` onto the provider's state vocabulary.
  const toCloudState = (s: ConnectionState): CloudConnectionState =>
    s === 'connected' ? 'connected' : s === 'connecting' ? 'connecting' : 'disconnected';

  let cb!: CloudConnectionCallbacks;
  let cloudState: CloudConnectionState = 'connected';

  const provider: RemoteConnectionProvider = {
    connect: () => Promise.resolve(),
    disconnect: () => {},
    getState: () => cloudState,
    sendFrame(frame: Uint8Array) {
      const out = demuxFrame(frame);
      if (out.kind !== 'json') return; // pane/control frames are not host requests here
      const msg = out.json as Record<string, unknown>;
      received.push(msg);
      const reply = hostReply(msg, host);
      if (!reply) return;
      // Handshake frames (notifications) reply synchronously so the client's
      // protocol state is set before the test's synchronous assertion; request
      // responses defer a microtask so the client's pending entry is registered
      // first (mirrors the real async DataChannel).
      const isHandshake = reply.method === '$/hello' || reply.method === '$/bye';
      if (isHandshake) cb.onFrame?.(encodeJsonFrame(reply));
      else queueMicrotask(() => cb.onFrame?.(encodeJsonFrame(reply)));
    },
  };

  const adapter = createCloudWebrtcTransportWith('conformance-device', (callbacks) => {
    cb = callbacks;
    return provider;
  });
  const rpc = new RpcClient(adapter);
  return {
    rpc,
    adapter,
    received,
    host,
    deliver: (frame) => cb.onFrame?.(encodeJsonFrame(frame)),
    deliverRaw: (paneId, bytes) => cb.onFrame?.(encodePaneFrame(paneId, bytes)),
    setState: (s) => {
      cloudState = toCloudState(s);
      cb.onState?.(cloudState);
    },
  };
}

const ARMS: ReadonlyArray<readonly [string, () => Harness]> = [
  ['LAN-WS', makeLanHarness],
  ['cloud-WebRTC', makeCloudHarness],
];

// ─────────────────────────────────────────────────────────────────────────────
// Shared conformance suite — runs identically on every transport arm (D6)
// ─────────────────────────────────────────────────────────────────────────────

describe.each(ARMS)('S7 conformance (%s) — D9 $/hello handshake', (_name, make) => {
  let h: Harness;
  beforeEach(() => {
    h = make();
  });

  it('sends the client $/hello with version + capabilities', () => {
    h.rpc.hello();
    const hello = h.received.find((m) => m.method === '$/hello');
    expect(hello).toEqual({
      jsonrpc: '2.0',
      method: '$/hello',
      params: { protocolVersion: 1, capabilities: [...CLIENT_CAPABILITIES] },
    });
  });

  it('stores the negotiated capability intersection from the host reply', () => {
    h.host.hostCapabilities = ['pane', 'invoke', 'fs']; // host serves a subset
    h.rpc.hello();
    expect(h.rpc.protocol?.protocolVersion).toBe(1);
    expect(h.rpc.protocol?.rejected).toBe(false);
    expect(h.rpc.hasCapability('fs')).toBe(true);
    expect(h.rpc.hasCapability('git')).toBe(false); // not advertised by host
  });

  it('notifies onNegotiated subscribers with the result', () => {
    let seen: { capabilities: Set<string> } | null = null;
    h.rpc.onNegotiated((p) => {
      seen = p;
    });
    h.rpc.hello();
    expect(seen).not.toBeNull();
    expect(seen!.capabilities.has('theme')).toBe(true);
  });

  it('marks the protocol rejected on a $/bye version mismatch', () => {
    h.host.hostProtocolVersion = 0; // host forces $/bye
    h.rpc.hello();
    expect(h.rpc.protocol?.rejected).toBe(true);
    expect(h.rpc.protocol?.reason).toBe('protocol-version-mismatch');
    expect(h.rpc.hasCapability('fs')).toBe(false); // rejected → no panels
  });

  it('does not surface $/hello as a regular notification', () => {
    let fired = false;
    h.rpc.onNotification('$/hello', () => {
      fired = true;
    });
    h.rpc.hello();
    expect(fired).toBe(false);
  });

  it('does not double-send $/hello on the happy path (idempotent per connection)', () => {
    h.rpc.hello();
    h.rpc.hello(); // second call is a no-op until a reconnect
    expect(h.received.filter((m) => m.method === '$/hello')).toHaveLength(1);
  });

  it('re-handshakes after a reconnect (SPA may have updated independently)', () => {
    h.rpc.hello();
    expect(h.received.filter((m) => m.method === '$/hello')).toHaveLength(1);
    h.setState('disconnected');
    h.setState('connecting');
    h.setState('connected');
    expect(h.received.filter((m) => m.method === '$/hello')).toHaveLength(2);
  });
});

describe.each(ARMS)('S7 conformance (%s) — JSON-RPC invoke round-trip', (_name, make) => {
  let h: Harness;
  beforeEach(() => {
    h = make();
    h.rpc.hello(); // upgrade to native JSON-RPC
  });

  it('round-trips a successful request natively after the handshake', async () => {
    h.host.responders.read_file = (params) => ({ echoed: params });
    const out = await h.rpc.request<{ echoed: unknown }>('read_file', { path: '/a' });
    expect(out).toEqual({ echoed: { path: '/a' } });
    const req = h.received.find((m) => m.method === 'read_file');
    expect(req).toMatchObject({ jsonrpc: '2.0', method: 'read_file', params: { path: '/a' } });
    expect(req).not.toHaveProperty('type'); // not the legacy {type:'invoke-request'} shape
  });

  it('propagates a structured error with full code + data (D-GM-2 fix)', async () => {
    h.host.responders.set_remote_enabled = () => {
      throw {
        code: 1001,
        message: 'command not available remotely: set_remote_enabled',
        data: { kind: 'capability_denied' },
      };
    };
    const p = h.rpc.request('set_remote_enabled', {});
    await expect(p).rejects.toBeInstanceOf(RpcRemoteError);
    await p.catch((e: RpcRemoteError) => {
      expect(e.code).toBe(1001); // NOT collapsed to -32603 INTERNAL_ERROR
      expect(e.data).toEqual({ kind: 'capability_denied' });
      expect(e.message).toContain('set_remote_enabled');
    });
  });

  it('preserves read-only / path-traversal structured codes end to end', async () => {
    h.host.responders.write_file = () => {
      throw { code: 1002, message: 'remote filesystem is read-only', data: { kind: 'read_only' } };
    };
    await h.rpc.request('write_file', { path: '/a', content: 'x' }).catch((e: RpcRemoteError) => {
      expect(e.code).toBe(1002);
      expect(e.data).toEqual({ kind: 'read_only' });
    });
  });

  it('correlates concurrent requests by id through the adapter', async () => {
    h.host.responders.a = () => 1;
    h.host.responders.b = () => 2;
    const [a, b] = await Promise.all([h.rpc.request<number>('a'), h.rpc.request<number>('b')]);
    expect(a).toBe(1);
    expect(b).toBe(2);
  });
});

describe.each(ARMS)('S7 conformance (%s) — $/cancel over the wire', (_name, make) => {
  let h: Harness;
  beforeEach(() => {
    h = make();
    h.rpc.hello();
  });

  it('cancel(id) emits a native $/cancel frame and rejects the request', async () => {
    const p = h.rpc.request('text_search', { query: 'x' }); // host never responds
    const id = (h.received.find((m) => m.method === 'text_search') as { id: number }).id;
    h.rpc.cancel(id);
    await expect(p).rejects.toThrow(/cancelled/);
    const cancel = h.received.find((m) => m.method === '$/cancel');
    expect(cancel).toEqual({ jsonrpc: '2.0', method: '$/cancel', params: { id } });
  });

  it('AbortSignal cancellation reaches the host as $/cancel', async () => {
    const ac = new AbortController();
    const p = h.rpc.request('text_search', { query: 'x' }, { signal: ac.signal });
    const id = (h.received.find((m) => m.method === 'text_search') as { id: number }).id;
    ac.abort();
    await expect(p).rejects.toThrow(/cancelled/);
    const cancel = h.received.find((m) => m.method === '$/cancel');
    expect(cancel).toEqual({ jsonrpc: '2.0', method: '$/cancel', params: { id } });
  });
});

describe.each(ARMS)('S7 conformance (%s) — pane bytes + notifications', (_name, make) => {
  it('forwards raw pane bytes through the adapter to the consumer', () => {
    const h = make();
    const got: { paneId: string; bytes: Uint8Array }[] = [];
    h.adapter.onPaneBytes((paneId, bytes) => got.push({ paneId, bytes }));
    const bytes = new Uint8Array([0x1b, 0x5b, 0x41]);
    h.deliverRaw('pane-7', bytes);
    expect(got).toEqual([{ paneId: 'pane-7', bytes }]);
  });

  it('host event pushes reach onNotification consumers (post-handshake)', () => {
    const h = make();
    h.rpc.hello();
    const handler = vi.fn();
    h.rpc.onNotification('fs-changed', handler);
    h.deliver({ jsonrpc: '2.0', method: 'fs-changed', params: { path: '/x' } });
    expect(handler).toHaveBeenCalledWith({ path: '/x' });
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// LAN-WS-only: pre-handshake legacy fallback (the cloud leg is native-only)
// ─────────────────────────────────────────────────────────────────────────────

describe('S7 conformance (LAN-WS) — pre-handshake legacy fallback', () => {
  it('sends invoke as the LEGACY envelope before the host $/hello reply', () => {
    const h = makeLanHarness();
    // No handshake yet → adapter still in legacy-translation mode.
    void h.rpc.request('read_file', { path: '/a' });
    const legacy = h.received.find((m) => m.type === 'invoke-request');
    expect(legacy).toEqual({ type: 'invoke-request', cmd: 'read_file', args: { path: '/a' }, _reqId: 1 });
  });

  it('a legacy invoke-result still resolves the request (old-host compatibility)', async () => {
    const h = makeLanHarness();
    const p = h.rpc.request<string>('read_file', { path: '/a' });
    const reqId = (h.received.find((m) => m.type === 'invoke-request') as { _reqId: number })._reqId;
    h.deliver({ type: 'invoke-result', _reqId: reqId, _result: 'hello' } as unknown as Record<string, unknown>);
    await expect(p).resolves.toBe('hello');
  });
});
