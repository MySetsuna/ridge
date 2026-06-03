// src/lib/remote/cloud/cloudPaneSource.test.ts
//
// Unit tests for the cloud host pane output source (D-GM-11). Covers:
//   • resolves active workspace then listens `pty-output-{ws}-{paneId}`
//   • encodes payload.data → UTF-8 bytes byte-identically (LAN RawBytes parity)
//   • non-string / empty data frames ignored
//   • unsubscribe AFTER subscribe-ready calls the real Tauri unlisten
//   • unsubscribe BEFORE subscribe-ready (race) still unlistens once ready
//   • workspace-resolution failure / listen failure → no stream, no throw
//   • end-to-end through CloudHostBridge: subscribe-pane → event → 0x10 frame

import { describe, it, expect, vi } from 'vitest';
import { createCloudPaneSource, type ListenFn } from './cloudPaneSource';
import { CloudHostBridge } from './cloudHostBridge';
import { demuxFrame, encodePaneFrame } from '../../transport/remote/cloudMux';

/** A deferred promise helper so a test can control resolution timing. */
function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe('createCloudPaneSource', () => {
  it('resolves the active workspace then listens the correct pty-output event', async () => {
    const unlisten = vi.fn();
    const listenSpy = vi.fn(async (_event: string) => unlisten);
    const listen = listenSpy as unknown as ListenFn;
    const source = createCloudPaneSource({
      listen,
      getActiveWorkspaceId: async () => 'ws-9',
      log: () => {},
    });

    source('pane-7', () => {});
    // Microtasks: getActiveWorkspaceId → listen.
    await vi.waitFor(() => expect(listenSpy).toHaveBeenCalledOnce());
    expect(listenSpy.mock.calls[0][0]).toBe('pty-output-ws-9-pane-7');
  });

  it('encodes payload.data into UTF-8 bytes byte-identically to LAN RawBytes', async () => {
    let fire: (e: { payload: { data: string } }) => void = () => {};
    const listen = vi.fn(async (_event: string, handler: (e: { payload: { data: string } }) => void) => {
      fire = handler;
      return () => {};
    }) as unknown as ListenFn;

    const out: Uint8Array[] = [];
    const source = createCloudPaneSource({
      listen,
      getActiveWorkspaceId: async () => 'ws',
      log: () => {},
    });
    source('p', (raw) => out.push(raw));
    await vi.waitFor(() => expect(listen).toHaveBeenCalledOnce());

    // A string with multibyte + ANSI escape — the same `data` the LAN path would
    // run through `data.as_bytes()`.
    const data = '\x1b[32mok\x1b[0m 你好';
    fire({ payload: { data } });

    expect(out).toHaveLength(1);
    expect(out[0]).toEqual(new TextEncoder().encode(data));
  });

  it('ignores frames with empty or non-string data', async () => {
    let fire: (e: { payload: unknown }) => void = () => {};
    const listen = vi.fn(async (_event: string, handler: (e: { payload: unknown }) => void) => {
      fire = handler;
      return () => {};
    }) as unknown as ListenFn;

    const out: Uint8Array[] = [];
    const source = createCloudPaneSource({
      listen,
      getActiveWorkspaceId: async () => 'ws',
      log: () => {},
    });
    source('p', (raw) => out.push(raw));
    await vi.waitFor(() => expect(listen).toHaveBeenCalledOnce());

    fire({ payload: { data: '' } });
    fire({ payload: { data: 123 } });
    fire({ payload: {} });
    fire({ payload: null });
    expect(out).toHaveLength(0);
  });

  it('unsubscribe after subscribe-ready calls the real Tauri unlisten', async () => {
    const unlisten = vi.fn();
    const listen = vi.fn(async () => unlisten) as unknown as ListenFn;
    const source = createCloudPaneSource({
      listen,
      getActiveWorkspaceId: async () => 'ws',
      log: () => {},
    });

    const unsub = source('p', () => {});
    await vi.waitFor(() => expect(listen).toHaveBeenCalledOnce());
    unsub();
    expect(unlisten).toHaveBeenCalledOnce();
  });

  it('unsubscribe while listen() is in-flight still unlistens once it resolves (race)', async () => {
    const unlisten = vi.fn();
    // listen() resolution is gated so we can unsubscribe while it is in-flight —
    // the leak scenario the source guards against (subscribe arrives after
    // cancellation, so the real unlisten must fire immediately on resolve).
    const listenGate = deferred<() => void>();
    const listenSpy = vi.fn(async (_event: string) => listenGate.promise);
    const listen = listenSpy as unknown as ListenFn;
    const source = createCloudPaneSource({
      listen,
      getActiveWorkspaceId: async () => 'ws',
      log: () => {},
    });

    const unsub = source('p', () => {});
    // Wait until listen() has been invoked (workspace resolved) but not resolved.
    await vi.waitFor(() => expect(listenSpy).toHaveBeenCalledOnce());
    // Unsubscribe while listen() is still pending.
    unsub();
    // Now let listen() resolve — the late subscription must be torn down at once.
    listenGate.resolve(unlisten);
    await vi.waitFor(() => expect(unlisten).toHaveBeenCalledOnce());
  });

  it('does not listen and does not throw when workspace resolution fails', async () => {
    const listen = vi.fn(async () => () => {}) as unknown as ListenFn;
    const log = vi.fn();
    const source = createCloudPaneSource({
      listen,
      getActiveWorkspaceId: async () => {
        throw new Error('no ws');
      },
      log,
    });

    expect(() => source('p', () => {})).not.toThrow();
    await vi.waitFor(() => expect(log).toHaveBeenCalled());
    expect(listen).not.toHaveBeenCalled();
  });

  it('does not throw when listen fails', async () => {
    const log = vi.fn();
    const listen = vi.fn(async () => {
      throw new Error('listen boom');
    }) as unknown as ListenFn;
    const source = createCloudPaneSource({
      listen,
      getActiveWorkspaceId: async () => 'ws',
      log,
    });

    expect(() => source('p', () => {})).not.toThrow();
    await vi.waitFor(() => expect(log).toHaveBeenCalled());
  });
});

describe('CloudHostBridge + cloudPaneSource (end-to-end pane stream)', () => {
  it('subscribe-pane → pty-output event → 0x10 frame back to controller', async () => {
    let fire: (e: { payload: { data: string } }) => void = () => {};
    const unlisten = vi.fn();
    const listenSpy = vi.fn(async (_event: string, handler: (e: { payload: { data: string } }) => void) => {
      fire = handler;
      return unlisten;
    });
    const listen = listenSpy as unknown as ListenFn;

    const sent: Uint8Array[] = [];
    const bridge = new CloudHostBridge({
      invoke: async () => null,
      sendFrame: (b) => sent.push(b),
      paneOutputSource: createCloudPaneSource({
        listen,
        getActiveWorkspaceId: async () => 'ws-1',
        log: () => {},
      }),
      log: () => {},
    });

    // Controller subscribes the pane (0x11 JSON notification).
    const { encodeJsonFrame } = await import('../../transport/remote/cloudMux');
    bridge.handleFrame(
      encodeJsonFrame({ jsonrpc: '2.0', method: 'subscribe-pane', params: { paneId: 'pane-A' } }),
    );

    await vi.waitFor(() => expect(listenSpy).toHaveBeenCalledOnce());
    expect(listenSpy.mock.calls[0][0]).toBe('pty-output-ws-1-pane-A');

    // The PTY emits some output.
    const data = 'PS C:\\> ';
    fire({ payload: { data } });

    // The host bridge must have sent exactly one 0x10 pane frame, byte-identical
    // to cloudMux.encodePaneFrame(paneId, utf8(data)).
    const panes = sent
      .map((f) => demuxFrame(f))
      .filter((r): r is { kind: 'pane'; paneId: string; bytes: Uint8Array } => r.kind === 'pane');
    expect(panes).toHaveLength(1);
    expect(panes[0].paneId).toBe('pane-A');
    expect(panes[0].bytes).toEqual(new TextEncoder().encode(data));
    expect(sent[sent.length - 1]).toEqual(encodePaneFrame('pane-A', new TextEncoder().encode(data)));

    // Unsubscribing the pane (bridge reset) tears down the Tauri listener.
    bridge.reset();
    await vi.waitFor(() => expect(unlisten).toHaveBeenCalledOnce());
  });
});
