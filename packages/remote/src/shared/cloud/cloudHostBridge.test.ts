// src/lib/remote/cloud/cloudHostBridge.test.ts
//
// Unit tests for the cloud host bridge (S4-host). Covers:
//   • demux routing (0x11 JSON → control; 0x10 PANE_RAW → ignored on host)
//   • JSON-RPC invoke routing: success result, structured error透传 (D-GM-2),
//     generic error → INTERNAL_ERROR(-32603)
//   • $/hello negotiation (capabilities intersection) + $/bye on version mismatch
//   • $/cancel best-effort abort (no late response after cancel)
//   • subscribe-pane → pane output pushed back as 0x10 frames (D-GM-7 layout)
//   • §5.5 key-binding verifier reject → $/bye + business frames dropped
//   • byte-exact parity with the controller-side cloudMux codec
//
// The test "controller" encodes its outbound frames with the SAME cloudMux
// codec the real browser controller uses, and decodes the host's replies with
// it too — so a passing test is a byte-level conformance proof between the two
// peers (they literally share encode/demux via cloudMux).

import { describe, it, expect, beforeEach, vi } from 'vitest';
import {
  CloudHostBridge,
  PANE_OUTPUT_FRAME_BYTES,
  negotiateHello,
  toJsonRpcError,
} from './cloudHostBridge';
import {
  CHANNEL,
  demuxFrame,
  encodeControlFrame,
  encodeJsonFrame,
  encodePaneFrame,
} from '@ridge/remote';
import { ed25519 } from '@noble/curves/ed25519.js';
import { bytesToBase64 } from './e2ee';

/**
 * A test rig that stands in for the controller + provider: it captures frames
 * the host sends (so the test can demux+assert them) and lets the test push
 * controller→host frames into the bridge.
 */
function makeRig(opts: {
  invoke?: (method: string, params?: Record<string, unknown>) => Promise<unknown>;
  paneOutputSource?: ConstructorParameters<typeof CloudHostBridge>[0]['paneOutputSource'];
  totpVerifier?: (code: string) => Promise<boolean>;
  totpBindVerifier?: (tag: Uint8Array) => Promise<boolean>;
  bindTranscript?: Uint8Array | null;
  hostEventSource?: ConstructorParameters<typeof CloudHostBridge>[0]['hostEventSource'];
  preauthorized?: boolean;
  log?: (level: 'warn' | 'error', message: string, detail?: unknown) => void;
} = {}) {
  const sent: Uint8Array[] = [];
  const invoke =
    opts.invoke ?? vi.fn(async () => null);
  const bridge = new CloudHostBridge({
    invoke,
    sendFrame: (b) => sent.push(b),
    paneOutputSource: opts.paneOutputSource,
    totpVerifier: opts.totpVerifier,
    totpBindVerifier: opts.totpBindVerifier,
    bindTranscript: opts.bindTranscript,
    hostEventSource: opts.hostEventSource,
    preauthorized: opts.preauthorized,
    log: opts.log ?? (() => {}), // silence diagnostics in tests
  });

  /** Push a JSON-RPC control frame as the controller would (0x11). */
  const sendJson = (value: unknown) => bridge.handleFrame(encodeJsonFrame(value));
  /** Push a §4 session-CONTROL frame as the controller would (0x12). */
  const sendControl = (value: unknown) => bridge.handleFrame(encodeControlFrame(value));
  /** Decode every host-sent JSON frame the test has captured so far. */
  const sentJson = () =>
    sent
      .map((f) => demuxFrame(f))
      .filter((r): r is { kind: 'json'; json: unknown } => r.kind === 'json')
      .map((r) => r.json as Record<string, unknown>);
  /** Decode every host-sent CONTROL frame (0x12). */
  const sentControl = () =>
    sent
      .map((f) => demuxFrame(f))
      .filter((r): r is { kind: 'control'; json: unknown } => r.kind === 'control')
      .map((r) => r.json as Record<string, unknown>);
  /** Decode every host-sent PANE_RAW frame. */
  const sentPane = () =>
    sent
      .map((f) => demuxFrame(f))
      .filter((r): r is { kind: 'pane'; paneId: string; bytes: Uint8Array } => r.kind === 'pane');

  return { bridge, sent, invoke, sendJson, sendControl, sentJson, sentControl, sentPane };
}

describe('CloudHostBridge — host lifecycle hooks', () => {
  it('forwards verified host events, sends preauthorized receipt, and unsubscribes on reset', () => {
    let emit: ((name: string, payload: unknown) => void) | undefined;
    let eventLive = true;
    const stop = vi.fn(() => { eventLive = false; });
    const rig = makeRig({
      preauthorized: true,
      hostEventSource: (callback) => {
        emit = callback;
        return stop;
      },
    });

    rig.bridge.onConnected();
    expect(rig.sentJson()).toContainEqual({ t: 'totp-result', ok: true, source: 'workspace-share' });
    const emitEvent = (name: string, payload: unknown) => {
      if (eventLive) emit?.(name, payload);
    };
    emitEvent('pane-meta-changed', { paneId: 'p1' });
    expect(rig.sentJson()).toContainEqual({ type: 'event', name: 'pane-meta-changed', payload: { paneId: 'p1' } });

    const beforeReset = rig.sent.length;
    rig.bridge.reset();
    expect(stop).toHaveBeenCalledOnce();
    emitEvent('pane-meta-changed', { paneId: 'p2' });
    expect(rig.sent).toHaveLength(beforeReset);
  });

  it('replaces and clears channel backpressure subscriptions', () => {
    const rig = makeRig();
    const firstStop = vi.fn();
    const secondStop = vi.fn();
    const first = { bufferedAmount: () => 0, onDrained: vi.fn(() => firstStop) };
    const second = { bufferedAmount: () => 0, onDrained: vi.fn(() => secondStop) };

    rig.bridge.attachChannelControl(first);
    rig.bridge.attachChannelControl(second);
    expect(firstStop).toHaveBeenCalledOnce();
    expect(second.onDrained).toHaveBeenCalledOnce();
    rig.bridge.reset();
    expect(secondStop).toHaveBeenCalledOnce();
  });
});

describe('CloudHostBridge — JSON-RPC invoke routing', () => {
  it('routes a request to invoke and replies with the result (0x11 round-trip)', async () => {
    const invoke = vi.fn(async (method: string, params?: Record<string, unknown>) => {
      expect(method).toBe('path_exists');
      expect(params).toEqual({ path: '/tmp/x' });
      return true;
    });
    const rig = makeRig({ invoke });

    rig.sendJson({ jsonrpc: '2.0', id: 7, method: 'path_exists', params: { path: '/tmp/x' } });
    await vi.waitFor(() => expect(rig.sentJson()).toHaveLength(1));

    expect(rig.sentJson()[0]).toEqual({ jsonrpc: '2.0', id: 7, result: true });
    // The reply rode the JSON channel byte.
    expect(rig.sent[0][0]).toBe(CHANNEL.JSON);
  });

  it('passes a structured {code,message,data} error through verbatim (D-GM-2)', async () => {
    const coreErr = {
      code: 1003,
      message: 'path traversal rejected',
      data: { kind: 'path_traversal' },
    };
    const rig = makeRig({ invoke: vi.fn(async () => Promise.reject(coreErr)) });

    rig.sendJson({ jsonrpc: '2.0', id: 'a', method: 'read_file', params: { path: '../etc' } });
    await vi.waitFor(() => expect(rig.sentJson()).toHaveLength(1));

    expect(rig.sentJson()[0]).toEqual({ jsonrpc: '2.0', id: 'a', error: coreErr });
  });

  it('maps a generic Error to JSON-RPC INTERNAL_ERROR(-32603)', async () => {
    const rig = makeRig({ invoke: vi.fn(async () => Promise.reject(new Error('boom'))) });

    rig.sendJson({ jsonrpc: '2.0', id: 1, method: 'git_fetch' });
    await vi.waitFor(() => expect(rig.sentJson()).toHaveLength(1));

    expect(rig.sentJson()[0]).toEqual({
      jsonrpc: '2.0',
      id: 1,
      error: { code: -32603, message: 'boom', data: { kind: 'internal' } },
    });
  });

  it('normalizes a null result to JSON null in the response', async () => {
    const rig = makeRig({ invoke: vi.fn(async () => undefined) });
    rig.sendJson({ jsonrpc: '2.0', id: 9, method: 'set_active_theme', params: { theme: 'dark' } });
    await vi.waitFor(() => expect(rig.sentJson()).toHaveLength(1));
    expect(rig.sentJson()[0]).toEqual({ jsonrpc: '2.0', id: 9, result: null });
  });

  it('coalesces retries of the same terminal input sequence', async () => {
    let resolveInvoke: (value: unknown) => void = () => {};
    const invoke = vi.fn(() => new Promise((resolve) => { resolveInvoke = resolve; }));
    const rig = makeRig({ invoke });
    const params = {
      workspaceId: 'ws-1',
      paneId: 'pane-1',
      data: 'abc',
      inputSourceId: 'controller-1',
      inputSequence: 7,
    };

    rig.sendJson({ jsonrpc: '2.0', id: 1, method: 'write_to_pty', params });
    rig.sendJson({ jsonrpc: '2.0', id: 2, method: 'write_to_pty', params });
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(1));
    resolveInvoke(null);
    await vi.waitFor(() => expect(rig.sentJson()).toHaveLength(2));

    expect(rig.sentJson()).toEqual([
      { jsonrpc: '2.0', id: 1, result: null },
      { jsonrpc: '2.0', id: 2, result: null },
    ]);
  });

  it('rejects one terminal input sequence reused with different data', async () => {
    let resolveInvoke: (value: unknown) => void = () => {};
    const invoke = vi.fn(() => new Promise((resolve) => { resolveInvoke = resolve; }));
    const rig = makeRig({ invoke });
    const base = {
      workspaceId: 'ws-1',
      paneId: 'pane-1',
      inputSourceId: 'controller-1',
      inputSequence: 7,
    };

    rig.sendJson({ jsonrpc: '2.0', id: 1, method: 'write_to_pty', params: { ...base, data: 'a' } });
    rig.sendJson({ jsonrpc: '2.0', id: 2, method: 'write_to_pty', params: { ...base, data: 'b' } });
    await vi.waitFor(() => expect(rig.sentJson()).toHaveLength(1));
    expect(rig.sentJson()[0]).toMatchObject({
      id: 2,
      error: { message: 'terminal input sequence reused with different data' },
    });
    expect(invoke).toHaveBeenCalledTimes(1);
    resolveInvoke(null);
  });
});

describe('CloudHostBridge — $/hello (D9) negotiation', () => {
  it('replies $/hello with the capability intersection', () => {
    const rig = makeRig();
    rig.sendJson({
      jsonrpc: '2.0',
      method: '$/hello',
      params: { protocolVersion: 1, capabilities: ['pane', 'invoke', 'fs'] },
    });
    expect(rig.sentJson()[0]).toEqual({
      jsonrpc: '2.0',
      method: '$/hello',
      params: { protocolVersion: 1, capabilities: ['pane', 'invoke', 'fs'] },
    });
  });

  it('advertises full host capabilities when the controller sends none', () => {
    const rig = makeRig();
    rig.sendJson({ jsonrpc: '2.0', method: '$/hello', params: { protocolVersion: 1 } });
    const reply = rig.sentJson()[0] as { params: { capabilities: string[] } };
    expect(reply.params.capabilities).toEqual([
      'pane',
      'invoke',
      'fs',
      'git',
      'search',
      'workspace',
      'theme',
      'teammate',
    ]);
  });

  it('replies $/bye on a lower protocol version', () => {
    const rig = makeRig();
    rig.sendJson({ jsonrpc: '2.0', method: '$/hello', params: { protocolVersion: 0 } });
    expect(rig.sentJson()[0]).toEqual({
      jsonrpc: '2.0',
      method: '$/bye',
      params: { reason: 'protocol-version-mismatch' },
    });
  });

  it('negotiateHello() matches the server.rs negotiate_hello shape', () => {
    // Mirrors src-tauri/src/remote/server.rs::negotiate_hello — keep in lock-step.
    expect(negotiateHello({ protocolVersion: 1, capabilities: ['git'] })).toEqual({
      jsonrpc: '2.0',
      method: '$/hello',
      params: { protocolVersion: 1, capabilities: ['git'] },
    });
  });
});

describe('CloudHostBridge — $/cancel', () => {
  it('aborts an in-flight invoke and suppresses its late response', async () => {
    let resolveInvoke: (v: unknown) => void = () => {};
    const invoke = vi.fn(
      () => new Promise((resolve) => { resolveInvoke = resolve; }),
    );
    const rig = makeRig({ invoke });

    rig.sendJson({ jsonrpc: '2.0', id: 42, method: 'text_search', params: { root: '/', query: 'x' } });
    // Cancel before the invoke resolves.
    rig.sendJson({ jsonrpc: '2.0', method: '$/cancel', params: { id: 42 } });
    // Now let the underlying invoke resolve late.
    resolveInvoke(['late', 'result']);
    await Promise.resolve();
    await Promise.resolve();

    // No response frame should have been sent for the cancelled request.
    expect(rig.sentJson()).toHaveLength(0);
  });
});

describe('CloudHostBridge — pane stream (D-GM-7 layout)', () => {
  it('pushes subscribed pane output back as 0x10 || paneIdLen || paneId || raw', () => {
    let emit: (raw: Uint8Array) => void = () => {};
    const paneOutputSource = vi.fn((
      paneId: string,
      workspaceId: string | undefined,
      onOutput: (raw: Uint8Array) => void,
    ) => {
      expect(paneId).toBe('pane-1');
      expect(workspaceId).toBe('workspace-1');
      emit = onOutput;
      return () => {};
    });
    const rig = makeRig({ paneOutputSource });

    rig.sendJson({
      jsonrpc: '2.0',
      method: 'subscribe-pane',
      params: { paneId: 'pane-1', workspaceId: 'workspace-1' },
    });
    expect(paneOutputSource).toHaveBeenCalledOnce();

    const raw = new TextEncoder().encode('hello pty');
    emit(raw);

    const panes = rig.sentPane();
    expect(panes).toHaveLength(1);
    expect(panes[0].paneId).toBe('pane-1');
    expect(panes[0].bytes).toEqual(raw);
    // Byte-exact parity: the frame equals what cloudMux.encodePaneFrame produces.
    expect(rig.sent[0]).toEqual(encodePaneFrame('pane-1', raw));
  });

  it('is idempotent across duplicate subscribe-pane', () => {
    const paneOutputSource = vi.fn(() => () => {});
    const rig = makeRig({ paneOutputSource });
    rig.sendJson({ jsonrpc: '2.0', method: 'subscribe-pane', params: { paneId: 'p' } });
    rig.sendJson({ jsonrpc: '2.0', method: 'subscribe-pane', params: { paneId: 'p' } });
    expect(paneOutputSource).toHaveBeenCalledOnce();
  });

  // §history-pull（2026-07-02）：订阅只登记 **live** fan-out；host **不再**推初始回放。
  // 历史由每个 controller 自己经 get_pane_scrollback_tail/before 拉（首屏小 + 滚顶分批），
  // 天然多控制端隔离（host 不广播 RIS，不会冲掉其它 controller 的屏幕）。
  it('does NOT push an initial scrollback replay on subscribe (history is controller-pulled)', () => {
    const invoke = vi.fn(async () => null);
    const paneOutputSource = vi.fn(() => () => {});
    const rig = makeRig({ invoke, paneOutputSource });
    rig.sendJson({ jsonrpc: '2.0', method: 'subscribe-pane', params: { paneId: 'pane-1' } });
    // Live source registered exactly once …
    expect(paneOutputSource).toHaveBeenCalledOnce();
    // … but the bridge itself pushes no replay/resync (controller pulls history).
    expect(invoke).not.toHaveBeenCalledWith('replay_pane_scrollback_raw', { paneId: 'pane-1' });
    expect(invoke).not.toHaveBeenCalledWith('resync_pane_raw', { paneId: 'pane-1' });
  });

  // 无 source（占位订阅）→ 不碰 invoke（host 端无流可放）。
  it('does not request a replay when no paneOutputSource is wired', () => {
    const invoke = vi.fn(async () => null);
    const rig = makeRig({ invoke }); // no paneOutputSource
    rig.sendJson({ jsonrpc: '2.0', method: 'subscribe-pane', params: { paneId: 'p' } });
    expect(invoke).not.toHaveBeenCalled();
  });

  it('registers subscribe-pane intent with no source wired (pane stream TODO)', () => {
    const rig = makeRig(); // no paneOutputSource
    rig.sendJson({ jsonrpc: '2.0', method: 'subscribe-pane', params: { paneId: 'p' } });
    // No frames sent (no real source), no throw.
    expect(rig.sent).toHaveLength(0);
  });

  it('contains a pane source failure without escaping the notification handler', () => {
    const log = vi.fn();
    const paneOutputSource = vi.fn(() => {
      throw new Error('source closed');
    });
    const rig = makeRig({ paneOutputSource, log });
    expect(() => rig.sendJson({
      jsonrpc: '2.0',
      method: 'subscribe-pane',
      params: { paneId: 'p' },
    })).not.toThrow();
    expect(log).toHaveBeenCalledWith('warn', expect.stringContaining('paneOutputSource(p) failed'), expect.any(Error));
  });
});

describe('CloudHostBridge — inbound demux edge cases', () => {
  it('ignores an inbound PANE_RAW frame (controller never sends PTY bytes)', () => {
    const invoke = vi.fn();
    const rig = makeRig({ invoke });
    rig.bridge.handleFrame(encodePaneFrame('p', new Uint8Array([1, 2, 3])));
    expect(invoke).not.toHaveBeenCalled();
    expect(rig.sent).toHaveLength(0);
  });

  it('drops a malformed JSON frame without throwing or replying', () => {
    const rig = makeRig();
    // 0x11 followed by invalid UTF-8 JSON.
    const bad = new Uint8Array([CHANNEL.JSON, 0x7b, 0x7b]); // "{{"
    expect(() => rig.bridge.handleFrame(bad)).not.toThrow();
    expect(rig.sentJson()).toHaveLength(0);
  });

  it('ignores a control frame missing jsonrpc:"2.0"', () => {
    const invoke = vi.fn();
    const rig = makeRig({ invoke });
    rig.sendJson({ id: 1, method: 'x' });
    expect(invoke).not.toHaveBeenCalled();
  });

  it('ignores an inbound response frame (host never sends requests)', () => {
    const rig = makeRig();
    rig.sendJson({ jsonrpc: '2.0', id: 1, result: 'unexpected' });
    expect(rig.sentJson()).toHaveLength(0);
  });
});

// §5.5 keyBindingVerifier 钩子已删（S1-F5 退役，生产从未接线）；0x11 bye 拒帧语义保留如下。
describe('CloudHostBridge — session termination (0x11 bye)', () => {
  // 概念 6：对端经 0x11 通道发来 $/bye（如 controller 验 host 签名失败）→ host 拒后续业务帧。
  it('inbound $/bye (signature-invalid) rejects the session: drops later business frames', async () => {
    const invoke = vi.fn(async () => 'should-not-run');
    const rig = makeRig({ invoke }); // 无 totp → verified=true，正常会放行业务帧

    rig.sendJson({ jsonrpc: '2.0', method: '$/bye', params: { reason: 'signature-invalid' } });
    // $/bye 后业务帧被丢弃 —— invoke 不执行、不回响应。
    rig.sendJson({ jsonrpc: '2.0', id: 1, method: 'path_exists', params: {} });
    await Promise.resolve();
    expect(invoke).not.toHaveBeenCalled();
    expect(rig.sentJson()).toHaveLength(0);
  });
});

describe('CloudHostBridge — reset', () => {
  it('aborts in-flight invokes and clears pane subscriptions', async () => {
    const unsub = vi.fn();
    const paneOutputSource = vi.fn(() => unsub);
    let resolveInvoke: (v: unknown) => void = () => {};
    const invoke = vi.fn(() => new Promise((r) => { resolveInvoke = r; }));
    const rig = makeRig({ invoke, paneOutputSource });

    rig.sendJson({ jsonrpc: '2.0', id: 1, method: 'text_search' });
    rig.sendJson({ jsonrpc: '2.0', method: 'subscribe-pane', params: { paneId: 'p' } });

    rig.bridge.reset();
    expect(unsub).toHaveBeenCalledOnce();

    // Late resolution after reset → no response sent.
    resolveInvoke('late');
    await Promise.resolve();
    expect(rig.sentJson()).toHaveLength(0);
  });
});

describe('CloudHostBridge — §4 cloud TOTP gate (CONTROL channel 0x12)', () => {
  it('rejects business invokes before verification (id → JSON-RPC error, invoke not run)', async () => {
    const invoke = vi.fn(async () => 'should-not-run');
    const rig = makeRig({ invoke, totpVerifier: vi.fn(async () => true) });

    rig.sendJson({ jsonrpc: '2.0', id: 9, method: 'path_exists' });
    await Promise.resolve();

    expect(invoke).not.toHaveBeenCalled();
    const reply = rig.sentJson().find((f) => f.id === 9);
    expect(reply).toBeDefined();
    expect((reply as { error?: { data?: { kind?: string } } }).error?.data?.kind).toBe(
      'totp-required',
    );
  });

  it('drops unverified pane subscriptions (no PTY stream registered)', async () => {
    const unsub = vi.fn();
    const paneOutputSource = vi.fn(() => unsub);
    const rig = makeRig({ paneOutputSource, totpVerifier: vi.fn(async () => true) });

    rig.sendJson({ jsonrpc: '2.0', method: 'subscribe-pane', params: { paneId: 'p' } });
    await Promise.resolve();

    expect(paneOutputSource).not.toHaveBeenCalled();
  });

  it('verifies a correct code over CONTROL, replies totp-result{ok:true}, then allows invokes', async () => {
    const totpVerifier = vi.fn(async (code: string) => code === '123456');
    const invoke = vi.fn(async () => 'ran');
    const rig = makeRig({ invoke, totpVerifier });

    rig.sendControl({ t: 'totp-verify', code: '123456' });
    await vi.waitFor(() => expect(rig.sentControl()).toHaveLength(1));
    expect(totpVerifier).toHaveBeenCalledWith('123456');
    expect(rig.sentControl()[0]).toEqual({ t: 'totp-result', ok: true });
    // The result rode the CONTROL channel byte, NOT the JSON-RPC byte.
    expect(rig.sent.find((f) => f[0] === CHANNEL.CONTROL)).toBeDefined();

    // Now a business invoke is allowed through.
    rig.sendJson({ jsonrpc: '2.0', id: 1, method: 'path_exists' });
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledOnce());
  });

  it('rejects a wrong code: totp-result{ok:false}, gate stays closed', async () => {
    const totpVerifier = vi.fn(async () => false);
    const invoke = vi.fn(async () => 'ran');
    const rig = makeRig({ invoke, totpVerifier });

    rig.sendControl({ t: 'totp-verify', code: '000000' });
    await vi.waitFor(() => expect(rig.sentControl()).toHaveLength(1));
    expect(rig.sentControl()[0]).toEqual({ t: 'totp-result', ok: false });

    rig.sendJson({ jsonrpc: '2.0', id: 1, method: 'path_exists' });
    await Promise.resolve();
    expect(invoke).not.toHaveBeenCalled();
  });

  it('treats a throwing verifier as a failed verification (ok:false, no throw)', async () => {
    const rig = makeRig({
      totpVerifier: vi.fn(async () => {
        throw new Error('verify boom');
      }),
    });
    rig.sendControl({ t: 'totp-verify', code: '123456' });
    await vi.waitFor(() => expect(rig.sentControl()).toHaveLength(1));
    expect(rig.sentControl()[0]).toEqual({ t: 'totp-result', ok: false });
  });

  it('with no verifier configured, business frames pass without TOTP (backward compat)', async () => {
    const invoke = vi.fn(async () => 'ran');
    const rig = makeRig({ invoke }); // no totpVerifier
    rig.sendJson({ jsonrpc: '2.0', id: 1, method: 'path_exists' });
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledOnce());
  });

  it('re-arms the gate on reset (reconnect requires fresh TOTP)', async () => {
    const totpVerifier = vi.fn(async () => true);
    const invoke = vi.fn(async () => 'ran');
    const rig = makeRig({ invoke, totpVerifier });

    rig.sendControl({ t: 'totp-verify', code: '123456' });
    await vi.waitFor(() => expect(rig.sentControl()).toHaveLength(1));

    rig.bridge.reset();

    // After reset, business frames are gated again.
    rig.sendJson({ jsonrpc: '2.0', id: 2, method: 'path_exists' });
    await Promise.resolve();
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe('CloudHostBridge — §4 TOTP brute-force lockout (audit #3)', () => {
  it('locks out after 5 failed totp-verify and stops calling the verifier', async () => {
    const totpVerifier = vi.fn(async () => false); // always wrong
    const rig = makeRig({ totpVerifier });

    // 5 failed attempts: each returns ok:false; the 5th flips the lock on.
    for (let i = 0; i < 5; i++) {
      rig.sendControl({ t: 'totp-verify', code: '000000' });
      // eslint-disable-next-line no-await-in-loop
      await vi.waitFor(() => expect(rig.sentControl()).toHaveLength(i + 1));
    }
    expect(totpVerifier).toHaveBeenCalledTimes(5);
    // The 5th reply already carries locked:true (failures hit the cap).
    expect(rig.sentControl()[4]).toEqual({ t: 'totp-result', ok: false, locked: true });

    // A 6th attempt is rejected WITHOUT invoking the verifier (brute-force closed).
    rig.sendControl({ t: 'totp-verify', code: '111111' });
    await vi.waitFor(() => expect(rig.sentControl()).toHaveLength(6));
    expect(totpVerifier).toHaveBeenCalledTimes(5); // unchanged — not consulted
    expect(rig.sentControl()[5]).toEqual({ t: 'totp-result', ok: false, locked: true });
  });

  it('does not consume attempts once verified (idempotent pass)', async () => {
    const totpVerifier = vi.fn(async (code: string) => code === '123456');
    const rig = makeRig({ totpVerifier });

    rig.sendControl({ t: 'totp-verify', code: '123456' }); // pass
    await vi.waitFor(() => expect(rig.sentControl()).toHaveLength(1));
    expect(rig.sentControl()[0]).toEqual({ t: 'totp-result', ok: true });

    // A later (e.g. duplicate) totp-verify still says ok and never re-runs the verifier.
    rig.sendControl({ t: 'totp-verify', code: 'whatever' });
    await vi.waitFor(() => expect(rig.sentControl()).toHaveLength(2));
    expect(rig.sentControl()[1]).toEqual({ t: 'totp-result', ok: true });
    expect(totpVerifier).toHaveBeenCalledTimes(1); // not consulted again
  });

  it('a correct code BEFORE the cap still passes (lockout only after N failures)', async () => {
    const totpVerifier = vi.fn(async (code: string) => code === '123456');
    const invoke = vi.fn(async () => 'ran');
    const rig = makeRig({ invoke, totpVerifier });

    // 4 wrong, then the correct one (still under the 5-failure cap).
    for (let i = 0; i < 4; i++) {
      rig.sendControl({ t: 'totp-verify', code: '000000' });
      // eslint-disable-next-line no-await-in-loop
      await vi.waitFor(() => expect(rig.sentControl()).toHaveLength(i + 1));
    }
    rig.sendControl({ t: 'totp-verify', code: '123456' });
    await vi.waitFor(() => expect(rig.sentControl()).toHaveLength(5));
    expect(rig.sentControl()[4]).toEqual({ t: 'totp-result', ok: true });

    // Verified → business invokes flow.
    rig.sendJson({ jsonrpc: '2.0', id: 1, method: 'path_exists' });
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledOnce());
  });

  it('clears the lockout on reset (reconnect gets fresh attempts)', async () => {
    const totpVerifier = vi.fn(async () => false);
    const rig = makeRig({ totpVerifier });

    for (let i = 0; i < 5; i++) {
      rig.sendControl({ t: 'totp-verify', code: '000000' });
      // eslint-disable-next-line no-await-in-loop
      await vi.waitFor(() => expect(rig.sentControl()).toHaveLength(i + 1));
    }
    expect(totpVerifier).toHaveBeenCalledTimes(5);

    rig.bridge.reset();

    // After reset, the verifier is consulted again (counter zeroed).
    rig.sendControl({ t: 'totp-verify', code: '000000' });
    await vi.waitFor(() => expect(rig.sentControl()).toHaveLength(6));
    expect(totpVerifier).toHaveBeenCalledTimes(6);
  });

  it('pushPaneOutput never emits pane bytes before TOTP verification (verified guard)', () => {
    const rig = makeRig({ totpVerifier: vi.fn(async () => true) });
    // Direct push before any verification → must be dropped (verified=false).
    rig.bridge.pushPaneOutput('pane-1', new TextEncoder().encode('secret pty'));
    expect(rig.sentPane()).toHaveLength(0);
  });

  it('pushPaneOutput emits once verified', async () => {
    const rig = makeRig({ totpVerifier: vi.fn(async () => true) });
    rig.sendControl({ t: 'totp-verify', code: '123456' });
    await vi.waitFor(() => expect(rig.sentControl()).toHaveLength(1));

    rig.bridge.pushPaneOutput('pane-1', new TextEncoder().encode('pty'));
    expect(rig.sentPane()).toHaveLength(1);
    expect(rig.sentPane()[0].paneId).toBe('pane-1');
  });

  it('pushPaneOutput emits with no verifier configured (backward compat, no gating)', () => {
    const rig = makeRig(); // no totpVerifier → verified=true from construction
    rig.bridge.pushPaneOutput('pane-1', new TextEncoder().encode('pty'));
    expect(rig.sentPane()).toHaveLength(1);
  });

  it('splits a burst before sealing so control frames can interleave', () => {
    const rig = makeRig();
    const raw = new Uint8Array(PANE_OUTPUT_FRAME_BYTES + 1);
    raw.forEach((_, index) => raw[index] = index & 0xff);
    rig.bridge.pushPaneOutput('pane-1', raw);
    const frames = rig.sentPane();
    expect(frames).toHaveLength(2);
    expect(frames[0].bytes).toHaveLength(PANE_OUTPUT_FRAME_BYTES);
    expect(frames[1].bytes).toHaveLength(1);
    expect([...frames[0].bytes, ...frames[1].bytes]).toEqual([...raw]);
  });

  // ── 零信任 #1：totp-bind（信道绑定 HMAC tag，明文码不上线）── 概念 5 ────────────
  // controller 在收到 host 0x02 后改发 `{t:'totp-bind', tag:base64(HMAC)}` 替代明文
  // `{t:'totp-verify', code}`。host 经注入的 totpBindVerifier（= verify_remote_totp_bind，
  // 本机种子 ±1 窗口重算比对）放行。与 totp-verify 共享 verified 门控 + 5 次锁定计数。

  /** 把任意 32 字节 tag base64 化（与 e2ee.ts bytesToBase64 同口径，btoa 友好）。 */
  const b64 = (bytes: Uint8Array) => btoa(String.fromCharCode(...bytes));
  const FAKE_TAG = new Uint8Array(32).fill(0xab);

  it('verifies a correct totp-bind tag, replies totp-result{ok:true}, decodes tag bytes', async () => {
    const totpBindVerifier = vi.fn(async (_tag: Uint8Array) => true);
    const invoke = vi.fn(async () => 'ok');
    const rig = makeRig({ invoke, totpBindVerifier });

    rig.sendControl({ t: 'totp-bind', tag: b64(FAKE_TAG) });
    await vi.waitFor(() => expect(rig.sentControl()).toHaveLength(1));
    // 校验器收到的是解码后的原始 tag 字节（不是 base64 串）。
    expect(totpBindVerifier).toHaveBeenCalledTimes(1);
    expect(Array.from(totpBindVerifier.mock.calls[0][0])).toEqual(Array.from(FAKE_TAG));
    expect(rig.sentControl()[0]).toEqual({ t: 'totp-result', ok: true });

    // 门控打开：业务 invoke 现在放行。
    rig.sendJson({ jsonrpc: '2.0', id: 1, method: 'read_file', params: {} });
    await vi.waitFor(() => expect(rig.sentJson()).toHaveLength(1));
    expect(rig.sentJson()[0].id).toBe(1);
  });

  it('rejects a wrong totp-bind tag: totp-result{ok:false}, gate stays closed', async () => {
    const totpBindVerifier = vi.fn(async () => false);
    const rig = makeRig({ totpBindVerifier });
    rig.sendControl({ t: 'totp-bind', tag: b64(FAKE_TAG) });
    await vi.waitFor(() => expect(rig.sentControl()).toHaveLength(1));
    expect(rig.sentControl()[0]).toEqual({ t: 'totp-result', ok: false });
  });

  it('totp-bind shares the 5-attempt lockout with totp-verify', async () => {
    const totpBindVerifier = vi.fn(async () => false);
    const rig = makeRig({ totpBindVerifier });
    for (let i = 0; i < 5; i++) {
      rig.sendControl({ t: 'totp-bind', tag: b64(FAKE_TAG) });
      await vi.waitFor(() => expect(rig.sentControl()).toHaveLength(i + 1));
    }
    expect(totpBindVerifier).toHaveBeenCalledTimes(5);
    expect(rig.sentControl()[4]).toEqual({ t: 'totp-result', ok: false, locked: true });

    // 锁定后不再调校验器。
    rig.sendControl({ t: 'totp-bind', tag: b64(FAKE_TAG) });
    await vi.waitFor(() => expect(rig.sentControl()).toHaveLength(6));
    expect(totpBindVerifier).toHaveBeenCalledTimes(5);
    expect(rig.sentControl()[5]).toEqual({ t: 'totp-result', ok: false, locked: true });
  });

  it('totp-bind with no bind verifier (but gating on) → ok:false, no gate bypass', async () => {
    // 门控由 totpVerifier 开启，但 controller 发来 totp-bind 而 host 未注入 bind 校验器：
    // 不能放行（否则等于绕过门控）→ 计为失败。
    const totpVerifier = vi.fn(async () => true);
    const rig = makeRig({ totpVerifier }); // 仅 totpVerifier，无 totpBindVerifier
    rig.sendControl({ t: 'totp-bind', tag: b64(FAKE_TAG) });
    await vi.waitFor(() => expect(rig.sentControl()).toHaveLength(1));
    expect(rig.sentControl()[0]).toEqual({ t: 'totp-result', ok: false });
  });

  it('totp-bind with no verifiers at all → ungated pass (backward compat)', async () => {
    const rig = makeRig(); // 无任何 TOTP 校验器 → 不门控
    rig.sendControl({ t: 'totp-bind', tag: b64(FAKE_TAG) });
    await vi.waitFor(() => expect(rig.sentControl()).toHaveLength(1));
    expect(rig.sentControl()[0]).toEqual({ t: 'totp-result', ok: true });
  });
});

describe('CloudHostBridge — DataChannel 背压 + 丢帧重同步 (弱网 P1)', () => {
  /** 可控的 fake DataChannel 背压接口（bufferedAmount + drain 触发）。 */
  function fakeChannel() {
    let buffered = 0;
    let drainCb: (() => void) | null = null;
    return {
      ctrl: {
        bufferedAmount: () => buffered,
        onDrained: (cb: () => void) => {
          drainCb = cb;
          return () => {
            drainCb = null;
          };
        },
      },
      setBuffered: (n: number) => {
        buffered = n;
      },
      drain: () => drainCb?.(),
    };
  }

  it('bufferedAmount 高于上水位(256KiB) → 丢 pane 帧；回落后仅向当前控制端发私有快照', async () => {
    const invoke = vi.fn(async () => ({ frame: '\x1bcRECOVERED' }));
    const rig = makeRig({ invoke });
    const ch = fakeChannel();
    rig.bridge.attachChannelControl(ch.ctrl);
    rig.sendJson({ jsonrpc: '2.0', method: 'subscribe-pane', params: { paneId: 'pane-1', active: true } });

    // 高水位（>256 KiB）→ 丢帧，未发出 pane 帧。
    ch.setBuffered(9 * 1024 * 1024);
    rig.bridge.pushPaneOutput('pane-1', new Uint8Array([1, 2, 3]));
    expect(rig.sentPane()).toHaveLength(0);

    // 缓冲回落 → 私有 canonical frame；不经广播式 Tauri resync。
    ch.setBuffered(0);
    ch.drain();
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith('get_pane_resync_frame', {
        paneId: 'pane-1',
        workspaceId: undefined,
        maxBytes: 256 * 1024,
      }),
    );
    expect(new TextDecoder().decode(rig.sentPane()[0].bytes)).toBe('\x1bcRECOVERED');
  });

  it('bufferedAmount 低于上水位 → 正常发 pane 帧', () => {
    const rig = makeRig();
    const ch = fakeChannel();
    rig.bridge.attachChannelControl(ch.ctrl);
    rig.sendJson({
      jsonrpc: '2.0',
      method: 'subscribe-pane',
      params: { paneId: 'pane-1', active: true },
    });
    ch.setBuffered(1024); // 远低于上水位
    rig.bridge.pushPaneOutput('pane-1', new Uint8Array([9, 9]));
    const panes = rig.sentPane();
    expect(panes).toHaveLength(1);
    expect(panes[0].paneId).toBe('pane-1');
    expect([...panes[0].bytes]).toEqual([9, 9]);
  });

  it('background stops at zero while active stays within the input latency budget', () => {
    const rig = makeRig();
    const ch = fakeChannel();
    rig.bridge.attachChannelControl(ch.ctrl);
    rig.sendJson({ jsonrpc: '2.0', method: 'subscribe-pane', params: { paneId: 'active', active: true } });
    rig.sendJson({ jsonrpc: '2.0', method: 'subscribe-pane', params: { paneId: 'background' } });
    ch.setBuffered(128 * 1024);
    rig.bridge.pushPaneOutput('background', new Uint8Array([1]));
    rig.bridge.pushPaneOutput('active', new Uint8Array([2]));
    expect(rig.sentPane().map((p) => p.paneId)).toEqual(['active']);
  });

  it('ordered channel admits at most one background frame before active traffic', () => {
    let buffered = 0;
    const sent: Uint8Array[] = [];
    const bridge = new CloudHostBridge({
      invoke: vi.fn(async () => null),
      sendFrame: (frame) => {
        sent.push(frame);
        buffered += frame.byteLength;
      },
    });
    bridge.attachChannelControl({
      bufferedAmount: () => buffered,
      onDrained: () => () => {},
    });
    bridge.handleFrame(encodeJsonFrame({
      jsonrpc: '2.0',
      method: 'subscribe-pane',
      params: { paneId: 'active', active: true },
    }));
    bridge.handleFrame(encodeJsonFrame({
      jsonrpc: '2.0',
      method: 'subscribe-pane',
      params: { paneId: 'background', active: false },
    }));

    bridge.pushPaneOutput('background', new Uint8Array([1]));
    bridge.pushPaneOutput('background', new Uint8Array([2]));
    bridge.pushPaneOutput('active', new Uint8Array([3]));

    expect(sent.map((frame) => demuxFrame(frame)))
      .toMatchObject([
        { kind: 'pane', paneId: 'background' },
        { kind: 'pane', paneId: 'active' },
      ]);
  });

  it('未注入 channel control → 不背压（向后兼容：总是直发）', () => {
    const rig = makeRig();
    rig.bridge.pushPaneOutput('pane-1', new Uint8Array([7]));
    expect(rig.sentPane()).toHaveLength(1);
  });

  it('drain 但本无背压丢帧 → 不请求 resync', () => {
    const invoke = vi.fn(async () => null);
    const rig = makeRig({ invoke });
    const ch = fakeChannel();
    rig.bridge.attachChannelControl(ch.ctrl);
    ch.drain(); // 没丢过帧
    expect(invoke).not.toHaveBeenCalled();
  });

  it('reset() 清背压待重同步集（重连后不残留旧 pane 的 resync 请求）', async () => {
    const invoke = vi.fn(async () => null);
    const rig = makeRig({ invoke });
    const ch = fakeChannel();
    rig.bridge.attachChannelControl(ch.ctrl);
    ch.setBuffered(9 * 1024 * 1024);
    rig.bridge.pushPaneOutput('pane-1', new Uint8Array([1]));
    rig.bridge.reset(); // 重连：清背压集
    ch.setBuffered(0);
    ch.drain();
    // 等一个微任务窗口，确认没有触发 resync（被 reset 清掉了）。
    await Promise.resolve();
    expect(invoke).not.toHaveBeenCalledWith('resync_pane_raw', { paneId: 'pane-1' });
  });
});

describe('toJsonRpcError (exported helper)', () => {
  it('passes a structured core error through, dropping undefined data', () => {
    expect(toJsonRpcError({ code: 1001, message: 'denied' })).toEqual({
      code: 1001,
      message: 'denied',
    });
  });

  it('wraps a string error as INTERNAL_ERROR', () => {
    expect(toJsonRpcError('plain string')).toEqual({
      code: -32603,
      message: 'plain string',
      data: { kind: 'internal' },
    });
  });
});

// ─── §7.4 TOTP trust-grant handshake ─────────────────────────────────────────
// Helpers to build a valid Ed25519 proof for a given keypair + nonce + transcript.
function buildTrustMsg(nonce: Uint8Array, transcript: Uint8Array): Uint8Array {
  const prefix = new TextEncoder().encode('ridge-totp-trust-v1');
  const msg = new Uint8Array(prefix.length + nonce.length + transcript.length);
  msg.set(prefix, 0);
  msg.set(nonce, prefix.length);
  msg.set(transcript, prefix.length + nonce.length);
  return msg;
}

describe('CloudHostBridge — §7.4 TOTP trust-grant handshake', () => {
  it('grants trust: valid Ed25519 proof + totp_trust_check=true → totp-trust-result{trusted:true}', async () => {
    // Arrange: generate a test keypair
    const { secretKey: privKey, publicKey: pubKey } = ed25519.keygen();
    const transcript = new Uint8Array([1, 2, 3, 4]);

    const invoke = vi.fn(async (method: string) => {
      if (method === 'totp_trust_check') return true;
      return null;
    });
    const { sendControl, sentControl } = makeRig({
      invoke,
      totpVerifier: async () => true, // TOTP verifier present (so gating is active)
      bindTranscript: transcript,
    });

    // Act: send totp-trust-hello
    sendControl({ t: 'totp-trust-hello', pub: bytesToBase64(pubKey) });
    await Promise.resolve(); // flush microtasks

    // The host should reply with a challenge
    const challenges = sentControl().filter((f) => f.t === 'totp-trust-challenge');
    expect(challenges).toHaveLength(1);
    const nonceB64 = challenges[0].nonce as string;
    const nonce = Uint8Array.from(atob(nonceB64), (c) => c.charCodeAt(0));
    expect(nonce.length).toBe(32);

    // Build a valid signature and send the proof
    const sig = ed25519.sign(buildTrustMsg(nonce, transcript), privKey);
    sendControl({ t: 'totp-trust-proof', sig: bytesToBase64(sig) });
    await Promise.resolve(); // flush microtasks

    // Assert: host sent totp-trust-result{trusted:true}
    const results = sentControl().filter((f) => f.t === 'totp-trust-result');
    expect(results).toHaveLength(1);
    expect(results[0].trusted).toBe(true);

    // totp_trust_check was called with the correct pub key
    expect(invoke).toHaveBeenCalledWith('totp_trust_check', { ctrlPubB64: bytesToBase64(pubKey) });
  });

  it('rejects bad sig: totp-trust-result{trusted:false}, gate stays closed', async () => {
    // Arrange
    const { secretKey: privKey, publicKey: pubKey } = ed25519.keygen();

    const { sendControl, sentControl } = makeRig({
      totpVerifier: async () => true,
    });

    // Send hello
    sendControl({ t: 'totp-trust-hello', pub: bytesToBase64(pubKey) });
    await Promise.resolve();

    // Send a proof with an all-zero (bad) signature
    const badSig = new Uint8Array(64); // all zeros, invalid
    sendControl({ t: 'totp-trust-proof', sig: bytesToBase64(badSig) });
    await Promise.resolve();

    const results = sentControl().filter((f) => f.t === 'totp-trust-result');
    expect(results).toHaveLength(1);
    expect(results[0].trusted).toBe(false);
  });

  it('ignores totp-trust-proof with no prior hello (missing nonce)', async () => {
    // Arrange: send a proof without a prior hello (no pending nonce)
    const { secretKey: privKey, publicKey: pubKey } = ed25519.keygen();
    const fakeSig = ed25519.sign(new Uint8Array(32), privKey);

    const { sendControl, sentControl } = makeRig({
      totpVerifier: async () => true,
    });

    // Act: send proof without hello first
    sendControl({ t: 'totp-trust-proof', sig: bytesToBase64(fakeSig) });
    await Promise.resolve();

    // Assert: no totp-trust-result should be emitted (frame is ignored)
    const results = sentControl().filter((f) => f.t === 'totp-trust-result');
    expect(results).toHaveLength(0);
    void pubKey; // suppress unused warning
  });
});

describe('CloudHostBridge — S1 兼容回落面（构造点矩阵门禁）', () => {
  it('bind-only 桥：明文 totp-verify 恒失败（无降级路径），门控保持关闭', async () => {
    const invoke = vi.fn(async () => 'ran');
    const { sendControl, sendJson, sentControl, sentJson } = makeRig({
      invoke,
      totpBindVerifier: async () => true,
      bindTranscript: new Uint8Array([1, 2, 3, 4]),
    });

    sendControl({ t: 'totp-verify', code: '123456' });
    await Promise.resolve();

    const results = sentControl().filter((f) => f.t === 'totp-result');
    expect(results).toHaveLength(1);
    expect(results[0].ok).toBe(false);

    // 门控仍关闭：业务 invoke 被拒且不执行。
    sendJson({ jsonrpc: '2.0', id: 9, method: 'get_pane_layout' });
    await Promise.resolve();
    expect(invoke).not.toHaveBeenCalled();
    const err = sentJson().find((f) => f.id === 9) as { error?: { data?: { kind?: string } } };
    expect(err?.error?.data?.kind).toBe('totp-required');
  });

  it('回落面钉死：host 无 bindTranscript 时 trust-proof 非“直接失败”——退化为无信道绑定签名 + 信任库裁决', async () => {
    // 现状语义（S1 审计发现）：config 原注释称“未注入 transcript 则 proof 直接失败”，
    // 实际 transcript=null ⇒ 签名消息退化为 prefix‖nonce（无信道绑定），能否通过由
    // totp_trust_check 决定。本测试钉死该回落面；fail-closed 改造（S1 退役目标）应
    // 要求 transcript 必在后才接受 trust-proof。
    const { secretKey, publicKey } = ed25519.keygen();
    const invoke = vi.fn(async (m: string) => (m === 'totp_trust_check' ? true : null));
    const { bridge, sendControl, sentControl } = makeRig({ invoke, totpVerifier: async () => true });

    sendControl({ t: 'totp-trust-hello', pub: bytesToBase64(publicKey) });
    await Promise.resolve();
    const nonceB64 = sentControl().find((f) => f.t === 'totp-trust-challenge')?.nonce as string;
    const nonce = Uint8Array.from(atob(nonceB64), (c) => c.charCodeAt(0));

    const sig = ed25519.sign(buildTrustMsg(nonce, new Uint8Array(0)), secretKey);
    sendControl({ t: 'totp-trust-proof', sig: bytesToBase64(sig) });
    await Promise.resolve();

    const result = sentControl().find((f) => f.t === 'totp-trust-result');
    expect(result?.trusted).toBe(true);
    expect(invoke).toHaveBeenCalledWith('totp_trust_check', { ctrlPubB64: bytesToBase64(publicKey) });
    // S1 遥测（F1）：无 transcript 的 proof 恰好计一次。
    expect(bridge.fallbackCounters).toEqual({
      trustProofWithTranscript: 0,
      trustProofWithoutTranscript: 1,
    });
  });

  it('transcript 不对称即拒：controller 带 transcript 签名而 host 无 → trusted:false', async () => {
    const { secretKey, publicKey } = ed25519.keygen();
    const invoke = vi.fn(async (m: string) => (m === 'totp_trust_check' ? true : null));
    const { sendControl, sentControl } = makeRig({ invoke, totpVerifier: async () => true });

    sendControl({ t: 'totp-trust-hello', pub: bytesToBase64(publicKey) });
    await Promise.resolve();
    const nonceB64 = sentControl().find((f) => f.t === 'totp-trust-challenge')?.nonce as string;
    const nonce = Uint8Array.from(atob(nonceB64), (c) => c.charCodeAt(0));

    const sig = ed25519.sign(buildTrustMsg(nonce, new Uint8Array([1, 2, 3, 4])), secretKey);
    sendControl({ t: 'totp-trust-proof', sig: bytesToBase64(sig) });
    await Promise.resolve();

    const result = sentControl().find((f) => f.t === 'totp-trust-result');
    expect(result?.trusted).toBe(false);
    expect(invoke).not.toHaveBeenCalledWith('totp_trust_check', expect.anything());
  });
});
