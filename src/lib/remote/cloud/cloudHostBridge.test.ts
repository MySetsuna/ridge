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
import { CloudHostBridge, negotiateHello, toJsonRpcError } from './cloudHostBridge';
import {
  CHANNEL,
  demuxFrame,
  encodeControlFrame,
  encodeJsonFrame,
  encodePaneFrame,
} from '../../transport/remote/cloudMux';

/**
 * A test rig that stands in for the controller + provider: it captures frames
 * the host sends (so the test can demux+assert them) and lets the test push
 * controller→host frames into the bridge.
 */
function makeRig(opts: {
  invoke?: (method: string, params?: Record<string, unknown>) => Promise<unknown>;
  paneOutputSource?: ConstructorParameters<typeof CloudHostBridge>[0]['paneOutputSource'];
  keyBindingVerifier?: (pub: Uint8Array) => boolean;
  totpVerifier?: (code: string) => Promise<boolean>;
} = {}) {
  const sent: Uint8Array[] = [];
  const invoke =
    opts.invoke ?? vi.fn(async () => null);
  const bridge = new CloudHostBridge({
    invoke,
    sendFrame: (b) => sent.push(b),
    paneOutputSource: opts.paneOutputSource,
    keyBindingVerifier: opts.keyBindingVerifier,
    totpVerifier: opts.totpVerifier,
    log: () => {}, // silence diagnostics in tests
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
    const paneOutputSource = vi.fn((paneId: string, onOutput: (raw: Uint8Array) => void) => {
      expect(paneId).toBe('pane-1');
      emit = onOutput;
      return () => {};
    });
    const rig = makeRig({ paneOutputSource });

    rig.sendJson({ jsonrpc: '2.0', method: 'subscribe-pane', params: { paneId: 'pane-1' } });
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

  it('registers subscribe-pane intent with no source wired (pane stream TODO)', () => {
    const rig = makeRig(); // no paneOutputSource
    rig.sendJson({ jsonrpc: '2.0', method: 'subscribe-pane', params: { paneId: 'p' } });
    // No frames sent (no real source), no throw.
    expect(rig.sent).toHaveLength(0);
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

describe('CloudHostBridge — §5.5 key-binding verifier', () => {
  it('accepts a verified peer key and processes business frames', async () => {
    const verifier = vi.fn(() => true);
    const invoke = vi.fn(async () => 'ok');
    const rig = makeRig({ keyBindingVerifier: verifier, invoke });

    expect(rig.bridge.verifyPeerKey(new Uint8Array(32))).toBe(true);
    rig.sendJson({ jsonrpc: '2.0', id: 1, method: 'path_exists' });
    await vi.waitFor(() => expect(rig.sentJson().some((f) => 'result' in f)).toBe(true));
  });

  it('rejects an unverified peer key: sends $/bye and drops business frames', async () => {
    const verifier = vi.fn(() => false);
    const invoke = vi.fn(async () => 'should-not-run');
    const rig = makeRig({ keyBindingVerifier: verifier, invoke });

    expect(rig.bridge.verifyPeerKey(new Uint8Array(32))).toBe(false);
    // $/bye was sent.
    expect(rig.sentJson()).toContainEqual({
      jsonrpc: '2.0',
      method: '$/bye',
      params: { reason: 'key-binding-failed' },
    });
    // Subsequent business frames are dropped — invoke never runs.
    rig.sendJson({ jsonrpc: '2.0', id: 1, method: 'path_exists' });
    await Promise.resolve();
    expect(invoke).not.toHaveBeenCalled();
  });

  it('treats a throwing verifier as a rejection', () => {
    const rig = makeRig({
      keyBindingVerifier: () => {
        throw new Error('verifier boom');
      },
    });
    expect(rig.bridge.verifyPeerKey(new Uint8Array(32))).toBe(false);
  });

  it('with no verifier configured, verifyPeerKey returns true (relay-trust)', () => {
    const rig = makeRig();
    expect(rig.bridge.verifyPeerKey(new Uint8Array(32))).toBe(true);
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

  it('bufferedAmount 高于上水位(8MiB) → 丢 pane 帧（不发）；回落 drain 后 invoke resync_pane_raw', async () => {
    const invoke = vi.fn(async () => null);
    const rig = makeRig({ invoke });
    const ch = fakeChannel();
    rig.bridge.attachChannelControl(ch.ctrl);

    // 高水位（>8 MiB）→ 丢帧，未发出 pane 帧。
    ch.setBuffered(9 * 1024 * 1024);
    rig.bridge.pushPaneOutput('pane-1', new Uint8Array([1, 2, 3]));
    expect(rig.sentPane()).toHaveLength(0);

    // 缓冲回落 → drain → 对背压期间丢帧的 pane 请求 host 重放（复用 desync→RIS+scrollback）。
    ch.setBuffered(0);
    ch.drain();
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith('resync_pane_raw', { paneId: 'pane-1' }),
    );
  });

  it('bufferedAmount 低于上水位 → 正常发 pane 帧', () => {
    const rig = makeRig();
    const ch = fakeChannel();
    rig.bridge.attachChannelControl(ch.ctrl);
    ch.setBuffered(1024); // 远低于上水位
    rig.bridge.pushPaneOutput('pane-1', new Uint8Array([9, 9]));
    const panes = rig.sentPane();
    expect(panes).toHaveLength(1);
    expect(panes[0].paneId).toBe('pane-1');
    expect([...panes[0].bytes]).toEqual([9, 9]);
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
