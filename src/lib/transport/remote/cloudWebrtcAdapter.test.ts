// src/lib/transport/remote/cloudWebrtcAdapter.test.ts
//
// Unit + integration tests for the cloud-WebRTC L1 adapter (handoff plan §5.3,
// contract §7). Covers:
//   • mux DEMUX (0x10 → onPaneBytes, 0x11 → onControl) and REMUX (send prefixes)
//   • CloudConnectionState → TransportState mapping (incl. the reconnect edge)
//   • the REAL L2 RpcClient stacked on the adapter + a fake provider host:
//     a JSON-RPC request round-trips, an error propagates with code/data, and a
//     reconnect rejects in-flight requests.

import { describe, it, expect, beforeEach, vi } from 'vitest';
import { CloudWebrtcAdapter, createCloudWebrtcTransportWith } from './cloudWebrtcAdapter';
import { CHANNEL, demuxFrame, encodeJsonFrame, encodePaneFrame } from './cloudMux';
import { RpcClient } from './rpcClient';
import { RpcRemoteError, RpcReconnectError } from './types';
import type {
  CloudConnectionCallbacks,
  CloudConnectionState,
  RemoteConnectionProvider,
} from '../../remote/cloud/connectionProvider';

/**
 * Fake cloud provider: the role-agnostic transport primitive the adapter wraps.
 * It records sent (encrypted-in-reality, plaintext-here) frames and lets a test
 * push inbound plaintext frames + drive the connection state, standing in for
 * `RidgeCloudProvider` + the remote host on the far side of the DataChannel.
 */
class FakeCloudProvider implements RemoteConnectionProvider {
  sent: Uint8Array[] = [];
  private cb: CloudConnectionCallbacks;
  private _state: CloudConnectionState = 'connected';
  connectedTo: string | null = null;
  disconnected = false;
  /** Optional hook fired with every sent frame (lets a test auto-reply as a host). */
  onSend: ((frame: Uint8Array) => void) | null = null;

  constructor(cb: CloudConnectionCallbacks, initial: CloudConnectionState = 'connected') {
    this.cb = cb;
    this._state = initial;
  }

  connect(deviceId: string): Promise<void> {
    this.connectedTo = deviceId;
    this.setState('connected');
    return Promise.resolve();
  }
  disconnect(): void {
    this.disconnected = true;
    this.setState('disconnected');
  }
  sendFrame(plaintext: Uint8Array): void {
    this.sent.push(plaintext);
    this.onSend?.(plaintext);
  }
  getState(): CloudConnectionState {
    return this._state;
  }

  // ── test drivers ──
  setState(s: CloudConnectionState): void {
    this._state = s;
    this.cb.onState?.(s);
  }
  /** Push an already-demuxable plaintext frame inbound (as the provider would
   *  after decrypting a DataChannel message). */
  deliverFrame(frame: Uint8Array): void {
    this.cb.onFrame?.(frame);
  }
  /** Convenience: push a JSON control frame inbound. */
  deliverJson(value: unknown): void {
    this.deliverFrame(encodeJsonFrame(value));
  }
  /** Convenience: push pane bytes inbound. */
  deliverPane(paneId: string, bytes: Uint8Array): void {
    this.deliverFrame(encodePaneFrame(paneId, bytes));
  }
  /** The most recently sent frame, demuxed. */
  lastDemuxed() {
    return demuxFrame(this.sent[this.sent.length - 1]);
  }
}

function wire(initial: CloudConnectionState = 'connected'): {
  provider: FakeCloudProvider;
  adapter: CloudWebrtcAdapter;
} {
  let provider!: FakeCloudProvider;
  const adapter = createCloudWebrtcTransportWith('my-laptop', (cb) => {
    provider = new FakeCloudProvider(cb, initial);
    return provider;
  });
  return { provider, adapter };
}

describe('CloudWebrtcAdapter — outbound remux', () => {
  let provider: FakeCloudProvider;
  let adapter: CloudWebrtcAdapter;

  beforeEach(() => {
    ({ provider, adapter } = wire());
  });

  it('sendControl prefixes the JSON-RPC envelope with 0x11', () => {
    adapter.sendControl({ jsonrpc: '2.0', id: 1, method: 'read_file', params: { path: '/a' } });
    expect(provider.sent[0][0]).toBe(CHANNEL.JSON);
    expect(provider.lastDemuxed()).toEqual({
      kind: 'json',
      json: { jsonrpc: '2.0', id: 1, method: 'read_file', params: { path: '/a' } },
    });
  });

  it('sendPaneBytes prefixes raw bytes with 0x10 || paneId', () => {
    const bytes = new Uint8Array([0x68, 0x69]);
    adapter.sendPaneBytes('pane-3', bytes);
    expect(provider.sent[0][0]).toBe(CHANNEL.PANE_RAW);
    expect(provider.lastDemuxed()).toEqual({ kind: 'pane', paneId: 'pane-3', bytes });
  });
});

describe('CloudWebrtcAdapter — inbound demux', () => {
  let provider: FakeCloudProvider;
  let adapter: CloudWebrtcAdapter;

  beforeEach(() => {
    ({ provider, adapter } = wire());
  });

  it('routes a 0x11 JSON frame to onControl', () => {
    const frames: unknown[] = [];
    adapter.onControl((f) => frames.push(f));
    provider.deliverJson({ jsonrpc: '2.0', id: 9, result: { ok: true } });
    expect(frames).toEqual([{ jsonrpc: '2.0', id: 9, result: { ok: true } }]);
  });

  it('routes a 0x10 PANE_RAW frame to onPaneBytes', () => {
    const got: { paneId: string; bytes: Uint8Array }[] = [];
    adapter.onPaneBytes((paneId, bytes) => got.push({ paneId, bytes }));
    const bytes = new Uint8Array([0x1b, 0x5b, 0x32, 0x4a]);
    provider.deliverPane('pane-1', bytes);
    expect(got).toEqual([{ paneId: 'pane-1', bytes }]);
  });

  it('does not cross the streams: a pane frame never reaches onControl', () => {
    const control: unknown[] = [];
    adapter.onControl((f) => control.push(f));
    provider.deliverPane('pane-1', new Uint8Array([1, 2, 3]));
    expect(control).toEqual([]);
  });

  it('drops a malformed JSON frame without throwing or tearing down', () => {
    const control: unknown[] = [];
    adapter.onControl((f) => control.push(f));
    expect(() =>
      provider.deliverFrame(new Uint8Array([CHANNEL.JSON, 0x7b, 0x7b])),
    ).not.toThrow();
    expect(control).toEqual([]);
  });

  it('ignores an unknown channel tag (forward-compat)', () => {
    const control: unknown[] = [];
    const panes: unknown[] = [];
    adapter.onControl((f) => control.push(f));
    adapter.onPaneBytes((p, b) => panes.push({ p, b }));
    provider.deliverFrame(new Uint8Array([0x42, 1, 2, 3]));
    expect(control).toEqual([]);
    expect(panes).toEqual([]);
  });

  it('ignores a non-object JSON payload on the control channel', () => {
    const control: unknown[] = [];
    adapter.onControl((f) => control.push(f));
    provider.deliverJson('just a string');
    provider.deliverJson(42);
    expect(control).toEqual([]);
  });
});

describe('CloudWebrtcAdapter — state mapping', () => {
  it('maps connected/connecting/handshaking/disconnected/error', () => {
    const { provider, adapter } = wire('connecting');
    expect(adapter.state()).toBe('connecting');
    provider.setState('handshaking');
    expect(adapter.state()).toBe('connecting'); // handshaking collapses to connecting
    provider.setState('connected');
    expect(adapter.state()).toBe('connected');
    provider.setState('error');
    expect(adapter.state()).toBe('error');
    provider.setState('disconnected');
    expect(adapter.state()).toBe('disconnected');
  });

  it('emits state transitions, collapsing connecting↔handshaking churn', () => {
    const { provider, adapter } = wire('connecting');
    const states: string[] = [];
    adapter.onStateChange((s) => states.push(s));
    provider.setState('handshaking'); // still "connecting" → no duplicate emit
    provider.setState('connected');
    provider.setState('disconnected');
    expect(states).toEqual(['connected', 'disconnected']);
  });

  it('connect() drives the provider to the target device', async () => {
    const { provider, adapter } = wire('disconnected');
    await adapter.connect();
    expect(provider.connectedTo).toBe('my-laptop');
  });

  it('close() disconnects the provider', () => {
    const { provider, adapter } = wire();
    adapter.close();
    expect(provider.disconnected).toBe(true);
  });
});

describe('CloudWebrtcAdapter + L2 RpcClient — end to end', () => {
  let provider: FakeCloudProvider;
  let adapter: CloudWebrtcAdapter;
  let rpc: RpcClient;

  beforeEach(() => {
    ({ provider, adapter } = wire());
    rpc = new RpcClient(adapter);
  });

  /** Minimal fake host: auto-replies to JSON-RPC requests the client sends.
   *  Hooks the provider's SEND path (a request goes out → the host answers it
   *  by pushing an inbound frame), mirroring the far side of the DataChannel. */
  function autoRespond(responders: Record<string, (params: unknown) => unknown>): void {
    provider.onSend = (frame) => {
      const out = demuxFrame(frame);
      if (out.kind !== 'json') return;
      const msg = out.json as { jsonrpc?: string; id?: number | string; method?: string; params?: unknown };
      if (msg.jsonrpc !== '2.0' || msg.id === undefined || typeof msg.method !== 'string') return;
      const responder = responders[msg.method];
      if (!responder) return;
      // Defer so the client finishes registering the pending entry before the
      // reply lands (the real transport is async anyway).
      queueMicrotask(() => {
        try {
          provider.deliverJson({ jsonrpc: '2.0', id: msg.id, result: responder(msg.params) });
        } catch (e) {
          provider.deliverJson({ jsonrpc: '2.0', id: msg.id, error: e });
        }
      });
    };
  }

  it('round-trips a JSON-RPC request through the mux', async () => {
    autoRespond({ read_file: (params) => ({ echoed: params }) });
    const out = await rpc.request<{ echoed: unknown }>('read_file', { path: '/a' });
    expect(out).toEqual({ echoed: { path: '/a' } });
    // The request left as a 0x11 JSON frame carrying the JSON-RPC envelope.
    const sentReq = demuxFrame(provider.sent[0]);
    expect(sentReq).toMatchObject({
      kind: 'json',
      json: { jsonrpc: '2.0', method: 'read_file', params: { path: '/a' } },
    });
  });

  it('propagates a structured host error with full code + data', async () => {
    autoRespond({
      set_remote_enabled: () => {
        throw { code: 1001, message: 'command not available remotely', data: { kind: 'capability_denied' } };
      },
    });
    const p = rpc.request('set_remote_enabled', {});
    await expect(p).rejects.toBeInstanceOf(RpcRemoteError);
    await p.catch((e: RpcRemoteError) => {
      expect(e.code).toBe(1001);
      expect(e.data).toEqual({ kind: 'capability_denied' });
    });
  });

  it('drives the D9 $/hello handshake over the mux', () => {
    rpc.hello();
    // The $/hello notification went out as a 0x11 JSON frame.
    const sent = demuxFrame(provider.sent[0]);
    expect(sent).toMatchObject({ kind: 'json', json: { jsonrpc: '2.0', method: '$/hello' } });
    // Host replies; client stores the negotiated intersection.
    provider.deliverJson({
      jsonrpc: '2.0',
      method: '$/hello',
      params: { protocolVersion: 1, capabilities: ['pane', 'invoke', 'fs'] },
    });
    expect(rpc.protocol?.protocolVersion).toBe(1);
    expect(rpc.hasCapability('fs')).toBe(true);
    expect(rpc.hasCapability('git')).toBe(false);
  });

  it('rejects in-flight requests on a reconnect edge (connected → disconnected)', async () => {
    // Host never responds → request stays in-flight.
    const p = rpc.request('text_search', { query: 'x' });
    expect(rpc.inFlight).toBe(1);
    provider.setState('disconnected'); // connected → disconnected edge
    await expect(p).rejects.toBeInstanceOf(RpcReconnectError);
    expect(rpc.inFlight).toBe(0);
  });

  it('delivers host event notifications to onNotification consumers', () => {
    const handler = vi.fn();
    rpc.onNotification('fs-changed', handler);
    provider.deliverJson({ jsonrpc: '2.0', method: 'fs-changed', params: { path: '/x' } });
    expect(handler).toHaveBeenCalledWith({ path: '/x' });
  });
});
