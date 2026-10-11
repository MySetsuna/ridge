// Unit tests for the host-side cloud pane raw-byte source (D-GM-11 / B2 wind half).
// Verifies the consumption contract with mocked Tauri invoke/listen: subscribe
// wires the listener + asks the host to stream; inbound base64 frames decode to
// raw bytes and reach onOutput; unsubscribe tears both down (incl. the
// listen-resolves-after-unsub race).

import { describe, it, expect, vi } from 'vitest';
import { base64ToBytes, makeCloudHostPaneSource, type ListenFn } from './cloudHostPaneSource';

function b64(bytes: number[]): string {
  let bin = '';
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin);
}

describe('base64ToBytes', () => {
  it('round-trips raw bytes', () => {
    expect(Array.from(base64ToBytes(b64([0x1b, 0x5b, 0x41])))).toEqual([0x1b, 0x5b, 0x41]);
  });
  it('returns empty on garbage without throwing', () => {
    expect(base64ToBytes('!!!not base64!!!')).toHaveLength(0);
  });
});

describe('makeCloudHostPaneSource', () => {
  it('uses the activation-scoped canonical terminal event and commands without raw fallback', async () => {
    const invoke = vi.fn().mockResolvedValue(undefined);
    let handler: ((e: { payload: unknown }) => void) | null = null;
    const unlisten = vi.fn();
    const listen: ListenFn = vi.fn(async (_event, h) => {
      handler = h as typeof handler;
      return unlisten;
    });
    const got: Uint8Array[] = [];
    const stop = makeCloudHostPaneSource({ invoke, listen })(
      'pane-7',
      'workspace-3',
      (raw) => got.push(raw),
      { activationId: 41 },
    );

    expect(listen).toHaveBeenCalledWith(
      'pane-terminal-workspace-3-pane-7-41',
      expect.any(Function),
    );
    await Promise.resolve();
    expect(invoke).toHaveBeenCalledWith('subscribe_pane_terminal', {
      paneId: 'pane-7',
      workspaceId: 'workspace-3',
      activationId: 41,
    });
    handler!({ payload: { b64: b64([0x13, 1, 2, 3]) } });
    expect(got).toEqual([new Uint8Array([0x13, 1, 2, 3])]);

    stop();
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
    expect(unlisten).toHaveBeenCalledOnce();
    expect(invoke).toHaveBeenCalledWith('unsubscribe_pane_terminal', {
      paneId: 'pane-7',
      workspaceId: 'workspace-3',
      activationId: 41,
    });
    expect(invoke).not.toHaveBeenCalledWith('subscribe_pane_raw', expect.anything());
  });

  it('subscribes only to the scoped canonical terminal listener', async () => {
    const invoke = vi.fn().mockResolvedValue(undefined);
    let handler: ((e: { payload: unknown }) => void) | null = null;
    const unlisten = vi.fn();
    const listen: ListenFn = vi.fn(async (_event, h) => {
      handler = h as typeof handler;
      return unlisten;
    });

    const source = makeCloudHostPaneSource({ invoke, listen });
    const got: Uint8Array[] = [];
    source('pane-7', 'workspace-3', (raw) => got.push(raw), { activationId: 41 });

    expect(listen).toHaveBeenCalledWith('pane-terminal-workspace-3-pane-7-41', expect.any(Function));
    await Promise.resolve(); // listener resolves before host subscription starts
    expect(invoke).toHaveBeenCalledWith('subscribe_pane_terminal', {
      paneId: 'pane-7',
      workspaceId: 'workspace-3',
      activationId: 41,
    });

    await Promise.resolve(); // let listen() resolve
    // Host pushes a raw frame → decoded bytes reach onOutput.
    handler!({ payload: { b64: b64([0x13, 0x68, 0x69]) } });
    expect(got).toEqual([new Uint8Array([0x13, 0x68, 0x69])]);
  });

  it('ignores frames without a string b64 payload', async () => {
    const invoke = vi.fn().mockResolvedValue(undefined);
    let handler: ((e: { payload: unknown }) => void) | null = null;
    const listen: ListenFn = vi.fn(async (_e, h) => {
      handler = h as typeof handler;
      return () => {};
    });
    const got: Uint8Array[] = [];
    makeCloudHostPaneSource({ invoke, listen })('p', 'ws', (r) => got.push(r), { activationId: 1 });
    await Promise.resolve();
    handler!({ payload: { b64: 123 } });
    handler!({ payload: {} });
    expect(got).toEqual([]);
  });

  it('unsubscribe tears down the listener + tells the host to stop', async () => {
    const invoke = vi.fn().mockResolvedValue(undefined);
    const unlisten = vi.fn();
    const listen: ListenFn = vi.fn(async () => unlisten);
    const unsub = makeCloudHostPaneSource({ invoke, listen })('p', 'ws', () => {}, { activationId: 1 });
    await Promise.resolve(); // listen resolves, unlisten stored

    unsub();
    await Promise.resolve(); // serialized stop follows subscribe
    await Promise.resolve();
    await Promise.resolve();
    expect(unlisten).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith('unsubscribe_pane_terminal', {
      paneId: 'p',
      workspaceId: 'ws',
      activationId: 1,
    });
  });

  it('handles unsubscribe BEFORE listen resolves (no leaked listener)', async () => {
    const invoke = vi.fn().mockResolvedValue(undefined);
    const unlisten = vi.fn();
    let resolveListen!: (u: () => void) => void;
    const listen: ListenFn = vi.fn(() => new Promise<() => void>((res) => (resolveListen = res)));

    const unsub = makeCloudHostPaneSource({ invoke, listen })('p', 'ws', () => {}, { activationId: 1 });
    unsub(); // unsubscribe while listen() is still pending
    resolveListen(unlisten); // listen finally resolves
    await Promise.resolve();
    expect(unlisten).toHaveBeenCalledTimes(1); // immediately detached, not leaked
  });

  it('serializes unsubscribe after an in-flight subscribe', async () => {
    let resolveSubscribe!: () => void;
    const invoke = vi.fn((cmd: string) =>
      cmd === 'subscribe_pane_terminal'
        ? new Promise<void>((resolve) => { resolveSubscribe = resolve; })
        : Promise.resolve(),
    );
    const listen: ListenFn = vi.fn(async () => () => {});
    const stop = makeCloudHostPaneSource({ invoke, listen })('p', 'ws', () => {}, { activationId: 1 });
    await Promise.resolve();
    stop();
    expect(invoke.mock.calls.map(([cmd]) => cmd)).toEqual(['subscribe_pane_terminal']);

    resolveSubscribe();
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
    expect(invoke.mock.calls.map(([cmd]) => cmd)).toEqual([
      'subscribe_pane_terminal',
      'unsubscribe_pane_terminal',
    ]);
  });

  it('releases a rejected registration, reports once and ignores late output', async () => {
    const invoke = vi.fn(async command => { if (command === 'subscribe_pane_terminal') throw new Error('command missing'); });
    let handler!: (e: { payload: unknown }) => void;
    const unlisten = vi.fn();
    const listen: ListenFn = vi.fn(async (_name, receive) => { handler = receive; return unlisten; });
    const onError = vi.fn(); const output = vi.fn();
    const stop = makeCloudHostPaneSource({ invoke, listen })('p', 'ws', output, { activationId: 1, onError });
    await vi.waitFor(() => expect(onError).toHaveBeenCalledOnce());
    expect(unlisten).toHaveBeenCalledOnce();
    handler({ payload: { b64: b64([0x13,1]) } });
    expect(output).not.toHaveBeenCalled();
    stop();
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('unsubscribe_pane_terminal', { paneId: 'p', workspaceId: 'ws', activationId: 1 }));
    expect(onError).toHaveBeenCalledOnce();
  });

  it('rejects missing canonical identity before registering any legacy source', () => {
    const invoke = vi.fn(); const listen = vi.fn();
    const source = makeCloudHostPaneSource({ invoke, listen });
    expect(() => source('p', undefined, () => {}, { activationId: 1 })).toThrow('canonical terminal binding required');
    expect(() => source('p', 'ws', () => {})).toThrow('canonical terminal binding required');
    expect(invoke).not.toHaveBeenCalled(); expect(listen).not.toHaveBeenCalled();
  });
});
