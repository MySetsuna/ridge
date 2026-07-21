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
    expect(base64ToBytes('!!!not base64!!!').length).toBe(0);
  });
});

describe('makeCloudHostPaneSource', () => {
  it('subscribes: asks the host to stream + wires the pane-raw listener', async () => {
    const invoke = vi.fn().mockResolvedValue(undefined);
    let handler: ((e: { payload: unknown }) => void) | null = null;
    const unlisten = vi.fn();
    const listen: ListenFn = vi.fn(async (_event, h) => {
      handler = h as typeof handler;
      return unlisten;
    });

    const source = makeCloudHostPaneSource({ invoke, listen });
    const got: Uint8Array[] = [];
    source('pane-7', (raw) => got.push(raw));

    expect(listen).toHaveBeenCalledWith('pane-raw-pane-7', expect.any(Function));
    expect(invoke).toHaveBeenCalledWith('subscribe_pane_raw', { paneId: 'pane-7' });

    await Promise.resolve(); // let listen() resolve
    // Host pushes a raw frame → decoded bytes reach onOutput.
    handler!({ payload: { b64: b64([0x68, 0x69]) } });
    expect(got).toEqual([new Uint8Array([0x68, 0x69])]);
  });

  it('ignores frames without a string b64 payload', async () => {
    const invoke = vi.fn().mockResolvedValue(undefined);
    let handler: ((e: { payload: unknown }) => void) | null = null;
    const listen: ListenFn = vi.fn(async (_e, h) => {
      handler = h as typeof handler;
      return () => {};
    });
    const got: Uint8Array[] = [];
    makeCloudHostPaneSource({ invoke, listen })('p', (r) => got.push(r));
    await Promise.resolve();
    handler!({ payload: { b64: 123 } });
    handler!({ payload: {} });
    expect(got).toEqual([]);
  });

  it('unsubscribe tears down the listener + tells the host to stop', async () => {
    const invoke = vi.fn().mockResolvedValue(undefined);
    const unlisten = vi.fn();
    const listen: ListenFn = vi.fn(async () => unlisten);
    const unsub = makeCloudHostPaneSource({ invoke, listen })('p', () => {});
    await Promise.resolve(); // listen resolves, unlisten stored

    unsub();
    expect(unlisten).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith('unsubscribe_pane_raw', { paneId: 'p' });
  });

  it('handles unsubscribe BEFORE listen resolves (no leaked listener)', async () => {
    const invoke = vi.fn().mockResolvedValue(undefined);
    const unlisten = vi.fn();
    let resolveListen!: (u: () => void) => void;
    const listen: ListenFn = vi.fn(() => new Promise<() => void>((res) => (resolveListen = res)));

    const unsub = makeCloudHostPaneSource({ invoke, listen })('p', () => {});
    unsub(); // unsubscribe while listen() is still pending
    resolveListen(unlisten); // listen finally resolves
    await Promise.resolve();
    expect(unlisten).toHaveBeenCalledTimes(1); // immediately detached, not leaked
  });

  it('swallows invoke failures (no stream must not crash the host)', async () => {
    const invoke = vi.fn().mockRejectedValue(new Error('command missing'));
    const listen: ListenFn = vi.fn(async () => () => {});
    expect(() => makeCloudHostPaneSource({ invoke, listen })('p', () => {})).not.toThrow();
    await Promise.resolve();
  });
});
