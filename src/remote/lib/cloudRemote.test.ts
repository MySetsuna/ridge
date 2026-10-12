import { describe, it, expect, vi, beforeEach } from 'vitest';

// ── Mock the tauriShim-aliased Tauri API (in the real mobile build these resolve
//    to the WebRTC bridge shims; here we drive them directly). vi.mock factories
//    are hoisted, so the mocks must be created via vi.hoisted to be referenceable. ──
const { invokeMock, listenMock, ChannelMock, controlMock } = vi.hoisted(() => {
  class ChannelMock {
    onmessage: (v: unknown) => void = () => {};
  }
  return { invokeMock: vi.fn(), listenMock: vi.fn(), ChannelMock, controlMock: vi.fn() };
});

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => Promise.resolve(invokeMock(...args)).then((result) => {
    if (args[0] === 'write_terminal_input' && result === undefined) {
      return { sequence: (args[1] as { input: { sequence: number } }).input.sequence, alreadyApplied: false };
    }
    return result;
  }),
  Channel: ChannelMock,
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: (...args: unknown[]) => listenMock(...args),
}));

import { CloudRemoteConnection } from './cloudRemote';
import { readFileSync } from 'node:fs';
import { CloudHostBridge } from '@ridge/remote/shared/cloud/cloudHostBridge';
import { createCloudWebrtcTransportWith } from '@ridge/remote/shared/transport/cloudWebrtcAdapter';
import type { CloudConnectionCallbacks } from '@ridge/remote/shared/transport/connectionProvider';
import type { PaneNode } from '$lib/types';
import {
  RpcClient,
  RpcTimeoutError,
  type ChannelTransport,
  type ControlFrame,
  type OutboundFrame,
  type PaneRef,
  type RpcRequestOptions,
  type WsMessage,
} from '@ridge/remote';

// Captured listen() handlers keyed by event name, so tests can fire host events.
let handlers: Record<string, (e: { payload: unknown }) => void>;
let disconnectSpy: ReturnType<typeof vi.fn>;
let verifyTotpSpy: ReturnType<typeof vi.fn>;

const LAYOUT: PaneNode = {
  type: 'split',
  id: 'root',
  direction: 'horizontal',
  ratios: [50, 50],
  children: [
    { type: 'leaf', id: 'pane-a', title: 'A', cwd: '/a', agent_state: 'busy', agent_id: 'agent-a' },
    { type: 'leaf', id: 'pane-b', agent_state: 'idle' },
  ],
};
const PANE = { workspaceId: 'ws1', paneId: 'pane-a' } as const;

function fakeHandle() {
  disconnectSpy = vi.fn();
  verifyTotpSpy = vi.fn(async () => true);
  return {
    adapter: {
      sendControl: controlMock,
      onTerminalFrame: (receive: (packet: Uint8Array) => void) => {
        handlers['terminal-frame'] = event => receive(event.payload as Uint8Array);
        return () => { delete handlers['terminal-frame']; };
      },
    } as never,
    hostDevice: 'dev',
    verifyTotp: verifyTotpSpy,
    tryTrustGrant: vi.fn(async () => false) as unknown as (timeoutMs?: number) => Promise<boolean>,
    disconnect: disconnectSpy,
  };
}

function rpcBackedBridge() {
  const sent: OutboundFrame[] = [];
  const controlListeners = new Set<(frame: ControlFrame) => void>();
  const transport: ChannelTransport = {
    sendControl(frame) {
      sent.push(frame);
      if (
        'id' in frame
        && 'method' in frame
        && frame.method !== 'write_terminal_input'
        && frame.method !== 'resize_pane'
      ) {
        queueMicrotask(() => {
          for (const listener of controlListeners) {
            listener({ jsonrpc: '2.0', id: frame.id, result: undefined });
          }
        });
      }
    },
    onControl(listener) {
      controlListeners.add(listener);
      return () => controlListeners.delete(listener);
    },
    sendPaneBytes() {},
    onPaneBytes() { return () => {}; },
    connect() {},
    close() {},
    state: () => 'connected',
    onStateChange: () => () => {},
    authState: () => 'authorized',
    onAuthChange: () => () => {},
  };
  const rpc = new RpcClient(transport, { defaultTimeoutMs: 0 });
  const injectedBridge = {
    invoke<T>(
      cmd: string,
      args: Record<string, unknown> = {},
      options: RpcRequestOptions = {},
    ) {
      return rpc.request<T>(cmd, args, options);
    },
    listen: async () => () => {},
    subscribePane: () => {},
    hasCapability: () => true,
    onCapabilitiesChanged: () => () => {},
  };
  return { injectedBridge, rpc, sent };
}

/** Flush pending microtasks/macrotasks so fire-and-forget async settles. */
const flush = () => new Promise((r) => setTimeout(r, 0));

beforeEach(() => {
  handlers = {};
  invokeMock.mockReset();
  controlMock.mockReset();
  listenMock.mockReset();
  invokeMock.mockImplementation(async (cmd: string) => {
    switch (cmd) {
      case 'get_active_workspace_id': return 'ws1';
      case 'get_pane_layout_for': return LAYOUT;
      case 'list_workspaces': return [{ id: 'ws1', name: 'One' }, { id: 'ws2', name: 'Two' }];
      case 'split_pane': return { pane_id: 'pane-new', initial_cwd: null };
      case 'create_workspace': return 'ws-new';
      case 'resume_agent_session': return { paneId: 'pane-resumed' };
      case 'get_active_theme_entry':
        return { id: 'dark1', type: 'dark', colors: { bg: '#000', accent: '#0f0' } };
      case 'get_theme_data':
        return { themes: [
          { id: 'dark1', type: 'dark', colors: { bg: '#000' } },
          { id: 'light1', type: 'light', colors: { bg: '#fff' } },
        ] };
      // §R-CLOUD-CONVERGE: host builds the complete resync frame (RIS + preamble +
      // tail) — the controller feeds `frame` verbatim. start_seq seeds the scroll-up
      // cursor; head_seq is the live/history seam.
      case 'get_pane_resync_frame':
        return { frame: '\x1bcHIST', start_seq: 100, at_oldest: false, head_seq: 116 };
      // §history-pull: seq-cursor scrollback. tail seeds the first screen (also the
      // version-skew fallback when get_pane_resync_frame is absent); before pages one
      // batch older then reports at_oldest so paging stops.
      case 'get_pane_scrollback_tail':
        return { bytes: 'HIST', start_seq: 100, at_oldest: false, head_seq: 116 };
      case 'get_pane_scrollback_before':
        return { bytes: 'OLDER', start_seq: 60, end_seq: 100, at_oldest: true, head_seq: 116 };
      default: return undefined;
    }
  });
  listenMock.mockImplementation(async (name: string, handler: (e: { payload: unknown }) => void) => {
    handlers[name] = handler;
    return () => { delete handlers[name]; };
  });
});

async function connected() {
  const conn = new CloudRemoteConnection(fakeHandle() as never);
  await conn.init();
  return conn;
}

describe('CloudRemoteConnection.init', () => {
  it('reads the active workspace and reaches connected', async () => {
    const conn = await connected();
    expect(invokeMock).toHaveBeenCalledWith('get_active_workspace_id');
    expect(conn.state()).toBe('connected');
    // Subscribes to host-side layout changes.
    expect(handlers['pane-tree-changed']).toBeTypeOf('function');
  });

  it('keeps the connection usable when optional bootstrap probes fail', async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'get_active_workspace_id' || cmd === 'get_active_theme_entry') {
        throw new Error(`${cmd} unavailable`);
      }
      return undefined;
    });
    listenMock.mockImplementation(async (name: string) => {
      if (name === 'pane-tree-changed' || name === 'pane-meta-changed') {
        throw new Error(`${name} unavailable`);
      }
      return () => {};
    });

    const conn = new CloudRemoteConnection(fakeHandle() as never);
    await conn.init();

    expect(conn.state()).toBe('connected');
    expect(conn.lastTheme()).toBeNull();
    conn.listPanes();
    await flush();
    expect(invokeMock).not.toHaveBeenCalledWith('get_pane_layout_for', expect.anything());
  });

  it('forwards only well-formed pane metadata events', async () => {
    const conn = await connected();
    const seen: Array<[PaneRef, string | null, string | null]> = [];
    conn.onMetadata((pane, title, cwd) => seen.push([pane, title, cwd]));

    handlers['pane-meta-changed']({ payload: null });
    handlers['pane-meta-changed']({ payload: { paneId: 42, title: 'bad' } });
    handlers['pane-meta-changed']({ payload: { paneId: 'pane-a', title: 'new', cwd: '/new' } });

    expect(seen).toEqual([[{ workspaceId: 'ws1', paneId: 'pane-a' }, 'new', '/new']]);
  });

  it('forwards the canonical grid and refresh owner from host events', async () => {
    const conn = await connected();
    const seen: unknown[] = [];
    conn.onPtyResize((pane, rows, cols, owner) => seen.push([pane, rows, cols, owner]));

    handlers['pty-resized']({ payload: { paneId: 'pane-a', rows: 31, cols: 101, owner: 'remote' } });
    handlers['pty-resized']({ payload: { workspaceId: 'ws2', paneId: 'pane-a', rows: 40, cols: 120 } });

    expect(seen).toEqual([[{ workspaceId: 'ws1', paneId: 'pane-a' }, 31, 101, 'remote']]);
    conn.disconnect();
  });
});

describe('CloudRemoteConnection panes', () => {
  it('shows active terminal source failure and ignores obsolete activations', async () => {
    const conn = await connected();
    const pane = { workspaceId: 'ws1', paneId: 'pane-a' };
    conn.subscribePane(pane, { activationId: 7, active: true });
    handlers['pane-terminal-error']({ payload: { ...pane, activationId: 6 } });
    expect(conn.lastFailure()).toBeNull();
    handlers['pane-terminal-error']({ payload: { ...pane, activationId: 7 } });
    expect(conn.lastFailure()).toMatchObject({ category: 'channel', message: expect.stringContaining('Terminal connection failed') });
    conn.disconnect();
    expect(handlers['pane-terminal-error']).toBeUndefined();
  });

  it('routes the authoritative snapshot through the real host bridge, adapter, client and WASM decoder', async () => {
    const { default: init, TerminalKernel } = await import('../../../packages/ridge-term/pkg/ridge_term.js');
    await init({ module_or_path: readFileSync(new URL('../../../packages/ridge-term/pkg/ridge_term_bg.wasm', import.meta.url)) });
    const packet = new Uint8Array(Buffer.from(readFileSync(new URL('../../../packages/remote/src/shared/cloud/signaling/fixtures/terminal/snapshot.hex', import.meta.url), 'utf8').trim(), 'hex'));
    let callbacks!: CloudConnectionCallbacks;
    let host!: CloudHostBridge;
    const adapter = createCloudWebrtcTransportWith('fixture-host', cb => {
      callbacks = cb;
      return {
        connect: async () => {}, disconnect: () => {}, getState: () => 'connected',
        sendFrame: frame => host.handleFrame(frame),
      };
    });
    host = new CloudHostBridge({
      preauthorized: true,
      invoke: async () => null,
      sendFrame: frame => callbacks.onFrame?.(frame),
      paneOutputSource: (_pane, _workspace, emit) => { emit(packet); return () => {}; },
    });
    const conn = new CloudRemoteConnection({ ...fakeHandle(), adapter } as never);
    const kernel = new TerminalKernel(2, 3, 256);
    const frames: Uint8Array[] = [];
    conn.onTerminalFrame(frame => {
      frames.push(frame);
      expect(kernel.applyRemoteFrame(frame, 'work', 'pane', 7)).toBe(true);
    });
    try {
      conn.activatePane({ workspaceId: 'work', paneId: 'pane' }, { activationId: 7 });
      expect(frames).toEqual([packet]);
      expect(kernel.terminalInputContext()).toEqual([{ workspaceId: 'work', paneId: 'pane' }, 'epoch-A', 7]);
      expect([kernel.cursorRow(), kernel.cursorCol()]).toEqual([1, 1]);
      expect(kernel.applyRemoteFrame(packet, 'work', 'pane', 8)).toBe(false);
    } finally { conn.disconnect(); host.reset(); adapter.dispose(); kernel.free(); }
  });
  it('listPanes flattens the tree into a panes message + metadata', async () => {
    const conn = await connected();
    const msgs: WsMessage[] = [];
    const metas: Array<[PaneRef, string | null, string | null]> = [];
    conn.onMessage((m) => msgs.push(m));
    conn.onMetadata((pane, title, cwd) => metas.push([pane, title, cwd]));

    conn.listPanes();
    await flush();

    const panesMsg = msgs.find((m) => m.type === 'panes');
    // Agent runtime state travels with the pane so mobile terminal chrome can
    // show the same working/idle cue as the desktop pane.
    expect(panesMsg).toEqual({ type: 'panes', workspaceId: 'ws1', panes: [
      { id: 'pane-a', title: 'A', cwd: '/a', isAgent: true, agentState: 'busy', agentId: 'agent-a' },
      { id: 'pane-b', title: undefined, cwd: undefined, isAgent: true, agentState: 'idle' },
    ] });
    expect(metas).toContainEqual([{ workspaceId: 'ws1', paneId: 'pane-a' }, 'A', '/a']);
    expect(metas).toContainEqual([{ workspaceId: 'ws1', paneId: 'pane-b' }, null, null]);
  });

  it('keeps a delayed pane snapshot scoped to the workspace that requested it', async () => {
    const conn = await connected();
    const msgs: WsMessage[] = [];
    let resolveLayout!: (layout: PaneNode) => void;
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'get_pane_layout_for') {
        return await new Promise<PaneNode>((resolve) => { resolveLayout = resolve; });
      }
      if (cmd === 'switch_workspace') return undefined;
      return undefined;
    });
    conn.onMessage((message) => msgs.push(message));

    conn.listPanes();
    await Promise.resolve();
    expect(await conn.switchWorkspace('ws2')).toBe(true);
    resolveLayout(LAYOUT);
    await flush();

    expect(msgs.at(-1)).toMatchObject({ type: 'panes', workspaceId: 'ws1' });
  });

  it('delivers canonical adapter packets directly and never reads raw history', async () => {
    const conn = await connected();
    const frames: Uint8Array[] = [];
    conn.onTerminalFrame(frame => frames.push(frame));
    conn.activatePane(PANE, { active: true, activationId: 29 });
    const packet = new Uint8Array([0x13, 4, 5, 6]);
    handlers['terminal-frame']({ payload: packet });
    expect(frames).toEqual([packet]);
    expect(controlMock).toHaveBeenCalledWith({ jsonrpc: '2.0', method: 'subscribe-pane', params: { ...PANE, active: true, activationId: 29 } });
    expect(listenMock.mock.calls.every(call => !String(call[0]).startsWith('pty-output'))).toBe(true);
    expect(invokeMock.mock.calls.some(call => /scrollback|resync_frame/.test(String(call[0])))).toBe(false);
    conn.disconnect();
    expect(handlers['terminal-frame']).toBeUndefined();
  });

  it('rejects subscriptions without a valid activation instead of replaying history', async () => {
    const conn = await connected();
    for (const activationId of [undefined, 0, -1, Number.NaN, 1.5]) {
      expect(() => conn.subscribePane(PANE, { activationId })).toThrow('terminal activation required');
    }
    expect(controlMock).not.toHaveBeenCalled();
    conn.disconnect();
  });

  it('changes activation on an already subscribed pane and preserves it on focus promotion', async () => {
    const conn = await connected();
    conn.subscribePane(PANE, { activationId: 7, active: false });
    conn.subscribePane(PANE, { activationId: 8, active: false });
    conn.subscribePane(PANE, { active: true });
    await flush();
    expect(controlMock.mock.calls.map(([frame]) => frame.params.activationId)).toEqual([7, 8, 8]);
    expect(controlMock.mock.calls.at(-1)![0].params.active).toBe(true);
    conn.disconnect();
  });

  it('unsubscribes the old composite pane before activating its replacement', async () => {
    const conn = await connected();
    const next = { workspaceId: 'ws2', paneId: PANE.paneId };
    conn.activatePane(PANE, { activationId: 7 });
    controlMock.mockClear();
    conn.activatePane(next, { activationId: 8 });
    expect(controlMock.mock.calls.map(([frame]) => [frame.method, frame.params])).toEqual([
      ['unsubscribe-pane', PANE],
      ['subscribe-pane', { ...next, active: true, activationId: 8 }],
    ]);
    conn.disconnect();
  });

  it('recovers only the current activation and discards late snapshots', async () => {
    const conn = await connected();
    const frames: Uint8Array[] = [];
    conn.onTerminalFrame(frame => frames.push(frame));
    conn.activatePane(PANE, { activationId: 7 });
    let resolve!: (encoded: string) => void;
    invokeMock.mockImplementationOnce(() => new Promise<string>(r => { resolve = r; }));
    conn.resyncPane(PANE);
    conn.activatePane(PANE, { activationId: 8 });
    const packet = new Uint8Array([0x13, 1, 2]);
    resolve(btoa(String.fromCharCode(...packet)));
    await flush();
    expect(frames).toEqual([]);
    invokeMock.mockResolvedValueOnce(btoa(String.fromCharCode(...packet)));
    conn.resyncPane(PANE);
    await flush();
    expect(frames).toEqual([packet]);
    expect(invokeMock).toHaveBeenCalledWith('get_terminal_snapshot', { ...PANE, activationId: 8 }, expect.objectContaining({ signal: expect.any(AbortSignal) }));
    conn.disconnect();
  });

  it('retries notification admission with bounded backoff and stops after disconnect', async () => {
    const conn = await connected();
    vi.useFakeTimers();
    controlMock.mockImplementation(() => { throw new Error('transport queue full'); });
    try {
      conn.subscribePane(PANE, { activationId: 7 });
      await vi.advanceTimersByTimeAsync(5000);
      expect(controlMock).toHaveBeenCalledTimes(5);
      conn.disconnect();
      await vi.advanceTimersByTimeAsync(5000);
      expect(controlMock).toHaveBeenCalledTimes(5);
    } finally { vi.useRealTimers(); }
  });

  it('fetches semantic history by absolute row range and rejects wrong tags', async () => {
    const conn = await connected();
    const request = { pane: PANE, terminalEpoch: 'epoch', activationId: 7, historyEpoch: 2, before: 100, maxRows: 128 };
    const packet = new Uint8Array([0x13, 1, 2]);
    invokeMock.mockResolvedValueOnce(btoa(String.fromCharCode(...packet)));
    expect(await conn.fetchTerminalHistory(request)).toEqual(packet);
    expect(invokeMock).toHaveBeenCalledWith('get_terminal_history', { request }, expect.objectContaining({ signal: expect.any(AbortSignal) }));
    invokeMock.mockResolvedValueOnce(btoa('raw VT'));
    await expect(conn.fetchTerminalHistory(request)).rejects.toThrow('invalid terminal history tag');
    conn.disconnect();
  });

  it('sendStdin writes to the pty', async () => {
    const conn = await connected();
    conn.sendStdin(PANE, 'ls\n', [{ ...PANE }, 'epoch', 1] as const);
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(invokeMock).toHaveBeenCalledWith('write_terminal_input', { input: { pane: { workspaceId: 'ws1', paneId: 'pane-a' }, terminalEpoch: 'epoch', activationId: 1, bytes: Array.from(new TextEncoder().encode('ls\n')), sequence: 1, sourceId: expect.any(String) } }, expect.objectContaining({ signal: expect.any(AbortSignal) }));
  });

  it('sends the first key immediately and coalesces later input without reordering', async () => {
    const conn = await connected();
    invokeMock.mockClear();
    let releaseFirst!: () => void;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd !== 'write_terminal_input') return Promise.resolve(undefined);
      if (!releaseFirst) return new Promise<void>((resolve) => { releaseFirst = resolve; });
      return Promise.resolve(undefined);
    });

    conn.sendStdin(PANE, 'a', [{ ...PANE }, 'epoch', 1] as const);
    // Zero input window: the first byte is admitted in the same turn.
    expect(invokeMock.mock.calls.filter((call) => call[0] === 'write_terminal_input')).toHaveLength(1);
    conn.sendStdin(PANE, 'b', [{ ...PANE }, 'epoch', 1] as const);
    await new Promise((resolve) => setTimeout(resolve, 10));
    conn.sendStdin(PANE, 'c', [{ ...PANE }, 'epoch', 1] as const);
    conn.sendStdin(PANE, 'd', [{ ...PANE }, 'epoch', 1] as const);
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(invokeMock.mock.calls.filter((call) => call[0] === 'write_terminal_input')).toHaveLength(1);

    releaseFirst();
    await new Promise((resolve) => setTimeout(resolve, 10));
    const writes = invokeMock.mock.calls.filter((call) => call[0] === 'write_terminal_input');
    expect(writes).toHaveLength(2);
    expect(writes[0][1]).toMatchObject({ input: { bytes: Array.from(new TextEncoder().encode('a')), sequence: 1 } });
    expect(writes[1][1]).toMatchObject({ input: { bytes: Array.from(new TextEncoder().encode('bcd')), sequence: 2, sourceId: writes[0][1].input.sourceId } });
  });

  it('keeps a 1,000-event input burst bounded after the immediate first RPC', async () => {
    const conn = await connected();
    invokeMock.mockClear();
    vi.useFakeTimers();
    try {
      for (let i = 0; i < 1_000; i += 1) conn.sendStdin(PANE, 'x', [{ ...PANE }, 'epoch', 1] as const);

      expect(invokeMock.mock.calls.filter((call) => call[0] === 'write_terminal_input')).toHaveLength(1);
      await vi.runAllTicks();
      for (let i = 0; i < 6; i++) await Promise.resolve();

      const writes = invokeMock.mock.calls.filter((call) => call[0] === 'write_terminal_input');
      expect(writes).toHaveLength(2);
      expect(writes[0][1]).toMatchObject({ input: { bytes: Array.from(new TextEncoder().encode('x')), sequence: 1 } });
      expect(writes[1][1]).toMatchObject({ input: { bytes: Array.from(new TextEncoder().encode('x'.repeat(999))), sequence: 2 } });
    } finally {
      conn.disconnect();
      vi.useRealTimers();
    }
  });

  it('retries a failed input batch with the same sequence after backoff', async () => {
    const conn = await connected();
    invokeMock.mockClear();
    let attempts = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd !== 'write_terminal_input') return Promise.resolve(undefined);
      attempts += 1;
      return attempts === 1
        ? Promise.reject(new RpcTimeoutError('write_terminal_input', 1_000))
        : Promise.resolve(undefined);
    });
    vi.useFakeTimers();
    try {
      conn.sendStdin(PANE, 'x', [{ ...PANE }, 'epoch', 1] as const);
      // The first request is immediate; let its rejection schedule the 100 ms
      // exponential-backoff timer before advancing the fake clock.
      await Promise.resolve();
      await Promise.resolve();
      expect(attempts).toBe(1);
      await vi.advanceTimersByTimeAsync(99);
      expect(attempts).toBe(1);
      await vi.advanceTimersByTimeAsync(1);
      expect(attempts).toBe(2);
      const writes = invokeMock.mock.calls.filter((call) => call[0] === 'write_terminal_input');
      expect(writes[0][1]).toMatchObject({ input: { bytes: Array.from(new TextEncoder().encode('x')), sequence: 1 } });
      expect(writes[1][1]).toMatchObject({ input: { bytes: Array.from(new TextEncoder().encode('x')), sequence: 1, sourceId: writes[0][1].input.sourceId } });
    } finally {
      vi.useRealTimers();
    }
  });

  it('pauses input retries after five consecutive timeouts until explicit pane recovery', async () => {
    const conn = await connected();
    invokeMock.mockClear();
    let attempts = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd !== 'write_terminal_input') return Promise.resolve(undefined);
      attempts += 1;
      return Promise.reject(new RpcTimeoutError('write_terminal_input', 1_000));
    });
    vi.useFakeTimers();
    try {
      conn.sendStdin(PANE, 'x', [{ ...PANE }, 'epoch', 1] as const);
      await vi.advanceTimersByTimeAsync(4);
      await vi.advanceTimersByTimeAsync(100);
      await vi.advanceTimersByTimeAsync(200);
      await vi.advanceTimersByTimeAsync(400);
      await vi.advanceTimersByTimeAsync(800);
      expect(attempts).toBe(5);

      await vi.advanceTimersByTimeAsync(30_000);
      expect(attempts).toBe(5);
      expect(conn.rpcSchedulingDiagnostics).toMatchObject({
        pausedLanes: 1,
        timeoutFailures: 5,
      });

      void conn.claimPane(PANE, 24, 80).catch(() => undefined);
      await Promise.resolve();
      expect(attempts).toBe(6);
    } finally {
      conn.disconnect();
      vi.useRealTimers();
    }
  });

  it('claimPane resizes the host pty and bumps the refresh seq', async () => {
    const conn = await connected();
    vi.useFakeTimers();
    try {
      const before = conn.lastRefreshSeq();
      const applied = conn.claimPane(PANE, 30, 100, 0, 0, 'remote');
      await vi.advanceTimersByTimeAsync(40);
      expect(invokeMock).toHaveBeenCalledWith('resize_pane', {
        workspaceId: 'ws1', paneId: 'pane-a', rows: 30, cols: 100, owner: 'remote',
      }, expect.objectContaining({ signal: expect.any(AbortSignal) }));
      expect(conn.lastRefreshSeq()).toBe(before + 1);
      await applied;
    } finally {
      conn.disconnect();
      vi.useRealTimers();
    }
  });

  it('coalesces resize bursts to one in-flight plus the latest value', async () => {
    const conn = await connected();
    invokeMock.mockClear();
    vi.useFakeTimers();
    let releaseFirst!: () => void;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd !== 'resize_pane') return Promise.resolve(undefined);
      if (!releaseFirst) return new Promise<void>((resolve) => { releaseFirst = resolve; });
      return Promise.resolve(undefined);
    });

    try {
      conn.refreshPane(PANE, 20, 80);
      await vi.advanceTimersByTimeAsync(40);
      conn.refreshPane(PANE, 21, 81);
      conn.refreshPane(PANE, 22, 82);
      expect(invokeMock.mock.calls.filter((c) => c[0] === 'resize_pane')).toHaveLength(1);

      releaseFirst();
      await Promise.resolve();
      await vi.advanceTimersByTimeAsync(40);
      const resizeCalls = invokeMock.mock.calls.filter((c) => c[0] === 'resize_pane');
      expect(resizeCalls).toHaveLength(2);
      expect(resizeCalls[1]).toEqual([
        'resize_pane',
        { workspaceId: 'ws1', paneId: 'pane-a', rows: 22, cols: 82 },
        expect.objectContaining({ signal: expect.any(AbortSignal) }),
      ]);

      conn.refreshPane(PANE, 22, 82);
      await vi.advanceTimersByTimeAsync(40);
      expect(invokeMock.mock.calls.filter((c) => c[0] === 'resize_pane')).toHaveLength(3);
      expect(invokeMock.mock.calls.filter((c) => c[0] === 'resize_pane')[2]).toEqual([
        'resize_pane',
        { workspaceId: 'ws1', paneId: 'pane-a', rows: 22, cols: 82 },
        expect.objectContaining({ signal: expect.any(AbortSignal) }),
      ]);
    } finally {
      conn.disconnect();
      vi.useRealTimers();
    }
  });

  it('coalesces 1,000 resize observations to one in-flight RPC plus the latest value', async () => {
    const conn = await connected();
    invokeMock.mockClear();
    vi.useFakeTimers();
    const before = conn.lastRefreshSeq();
    let releaseFirst!: () => void;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd !== 'resize_pane') return Promise.resolve(undefined);
      if (!releaseFirst) return new Promise<void>((resolve) => { releaseFirst = resolve; });
      return Promise.resolve(undefined);
    });

    try {
      conn.refreshPane(PANE, 20, 80);
      await vi.advanceTimersByTimeAsync(40);
      for (let i = 1; i < 1_000; i += 1) {
        conn.refreshPane(PANE, 20 + i, 80 + i);
      }
      expect(invokeMock.mock.calls.filter((call) => call[0] === 'resize_pane')).toHaveLength(1);
      expect(conn.lastRefreshSeq()).toBe(before + 1_000);

      releaseFirst();
      await Promise.resolve();
      await vi.advanceTimersByTimeAsync(40);
      const resizes = invokeMock.mock.calls.filter((call) => call[0] === 'resize_pane');
      expect(resizes).toHaveLength(2);
      expect(resizes[1][1]).toMatchObject({ rows: 1_019, cols: 1_079 });
    } finally {
      conn.disconnect();
      vi.useRealTimers();
    }
  });

  it('drops queued resize work when the pane closes', async () => {
    const conn = await connected();
    invokeMock.mockClear();
    vi.useFakeTimers();
    let releaseResize!: () => void;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'resize_pane') {
        return new Promise<void>((resolve) => { releaseResize = resolve; });
      }
      return Promise.resolve(undefined);
    });

    try {
      conn.refreshPane(PANE, 20, 80);
      await vi.advanceTimersByTimeAsync(40);
      conn.refreshPane(PANE, 30, 100);
      await conn.closePane(PANE);
      releaseResize();
      await Promise.resolve();
      await vi.advanceTimersByTimeAsync(40);

      expect(invokeMock.mock.calls.filter((c) => c[0] === 'resize_pane')).toHaveLength(1);
    } finally {
      conn.disconnect();
      vi.useRealTimers();
    }
  });

  it('createPane splits the first existing leaf', async () => {
    const conn = await connected();
    const id = await conn.createPane();
    expect(invokeMock).toHaveBeenCalledWith(
      'get_pane_layout_for',
      { workspaceId: 'ws1' },
    );
    expect(invokeMock).toHaveBeenCalledWith(
      'split_pane',
      { workspaceId: 'ws1', paneId: 'pane-a', direction: 'horizontal' },
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
    expect(id).toBe('pane-new');
  });

  it('closePane closes and stops streaming the pane', async () => {
    const conn = await connected();
    const frames = vi.fn();
    conn.onTerminalFrame(frames);
    conn.subscribePane(PANE, { activationId: 1 });
    await flush();
    expect(controlMock).toHaveBeenCalledWith(expect.objectContaining({ method: 'subscribe-pane', params: expect.objectContaining(PANE) }));

    const ok = await conn.closePane(PANE);
    expect(ok).toBe(true);
    expect(invokeMock).toHaveBeenCalledWith('close_pane', { workspaceId: 'ws1', paneId: 'pane-a' });
    handlers['terminal-frame']({ payload: new Uint8Array([0x13, 1]) });
    expect(frames).not.toHaveBeenCalled();
    expect(conn.sendStdin(PANE, 'after-close', [PANE, 'epoch', 1])).toBe(false);
    conn.disconnect();
  });

  it('pruneOutputs releases listeners for panes the host dropped', async () => {
    const conn = await connected();
    conn.subscribePane(PANE, { activationId: 1 });
    await flush();
    conn.pruneOutputs(new Set(['pane-b'])); // pane-a no longer live
    expect(handlers['pty-output-ws1-pane-a']).toBeUndefined();
  });
});

describe('CloudRemoteConnection workspaces', () => {
  it('listWorkspaces maps the active flag from get_active_workspace_id', async () => {
    const conn = await connected();
    const { workspaces } = await conn.listWorkspaces();
    expect(workspaces).toEqual([
      { id: 'ws1', name: 'One', active: true },
      { id: 'ws2', name: 'Two', active: false },
    ]);
  });

  it('surfaces workspace discovery failures instead of pretending the host is empty', async () => {
    const conn = await connected();
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'list_workspaces') throw new Error('workspace discovery failed');
      if (cmd === 'get_active_workspace_id') return 'ws1';
      return undefined;
    });

    await expect(conn.listWorkspaces()).rejects.toThrow('workspace discovery failed');
  });

  it('surfaces pane discovery failures instead of erasing the remote tree', async () => {
    const conn = await connected();
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'get_pane_layout_for') throw new Error('pane discovery failed');
      return undefined;
    });

    await expect(conn.listWorkspacePanes('ws1')).rejects.toThrow('pane discovery failed');
  });

  it('surfaces workspace create failures instead of turning them into a no-op', async () => {
    const conn = await connected();
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'create_workspace') throw new Error('workspace create failed');
      return undefined;
    });

    await expect(conn.createWorkspace('new')).rejects.toThrow('workspace create failed');
  });

  it('switchWorkspace updates the active ws used for pane events', async () => {
    const conn = await connected();
    expect(await conn.switchWorkspace('ws2')).toBe(true);
    expect(invokeMock).toHaveBeenCalledWith('switch_workspace', { workspaceId: 'ws2' });
    // Subsequent pane subscription targets the new workspace's event name.
    conn.subscribePane({ workspaceId: 'ws2', paneId: 'pane-a' }, { activationId: 1 });
    await flush();
    expect(controlMock).toHaveBeenCalledWith(expect.objectContaining({ method: 'subscribe-pane', params: expect.objectContaining({ workspaceId: 'ws2', paneId: 'pane-a' }) }));
  });
});

describe('CloudRemoteConnection theme', () => {
  it('init reads the host active theme into lastTheme', async () => {
    const conn = await connected();
    expect(invokeMock).toHaveBeenCalledWith('get_active_theme_entry');
    expect(conn.lastTheme()).toEqual({
      id: 'dark1', themeType: 'dark', colors: { bg: '#000', accent: '#0f0' },
    });
  });

  it('cycleTheme applies the next catalog theme locally without mutating the host', async () => {
    const conn = await connected();
    const seen: Array<{ colors: Record<string, string>; type: string }> = [];
    conn.onTheme((colors, type) => seen.push({ colors, type }));
    conn.cycleTheme('dark1'); // current dark1 → next light1
    await flush();
    expect(seen.at(-1)).toEqual({ colors: { bg: '#fff' }, type: 'light' });
    expect(conn.lastTheme()?.id).toBe('light1');
    // §theme-isolation: cycling must NOT re-skin the host / other viewers.
    expect(invokeMock).not.toHaveBeenCalledWith('set_active_theme', expect.anything());
  });
});

describe('CloudRemoteConnection reconnect', () => {
  it('surfaces a drop then re-auths + fires onReconnect on recovery', async () => {
    const conn = await connected();
    conn.setVerifiedCode('123456');
    let reconnected = 0;
    conn.onReconnect(() => reconnected++);

    conn.notifyState('disconnected');
    expect(conn.state()).toBe('disconnected');

    conn.notifyState('connected'); // recovery edge
    await flush();
    expect(verifyTotpSpy).toHaveBeenCalledWith('123456'); // compatibility fallback after trust miss
    expect(reconnected).toBe(1); // MainApp resync triggered
    expect(conn.state()).toBe('connected');
  });

  it('surfaces error when re-auth fails (stale code after a long outage)', async () => {
    const conn = await connected();
    verifyTotpSpy.mockResolvedValue(false);
    conn.setVerifiedCode('000000');
    conn.notifyState('disconnected');
    conn.notifyState('connected');
    await flush();
    expect(conn.state()).toBe('error');
  });

  it('preserves an admitted input batch and resumes it with the same identity after re-auth', async () => {
    vi.useFakeTimers();
    const { injectedBridge, rpc, sent } = rpcBackedBridge();
    const conn = new CloudRemoteConnection(fakeHandle() as never, injectedBridge);
    try {
      await conn.init();
      conn.setVerifiedCode('123456');
      conn.sendStdin(PANE, 'kept', [{ ...PANE }, 'epoch', 1] as const);
      await vi.advanceTimersByTimeAsync(4);
      const first = sent.find(
        (frame) => 'method' in frame && frame.method === 'write_terminal_input',
      );
      expect(first).toBeDefined();

      conn.notifyState('disconnected');
      conn.notifyState('connected');
      for (let i = 0; i < 8; i += 1) await Promise.resolve();

      const writes = sent.filter(
        (frame) => 'method' in frame && frame.method === 'write_terminal_input',
      );
      expect(writes).toHaveLength(2);
      expect(writes[1]).toMatchObject({ params: (first as { params: unknown }).params });
      expect(rpc.inFlight).toBe(1);
    } finally {
      conn.disconnect();
      rpc.dispose();
      vi.useRealTimers();
    }
  });

  it('rejects branded Agent launch without issuing an RPC', async () => {
    const conn = await connected();
    const calls = invokeMock.mock.calls.length;
    await expect(conn.resumeAgentSession('ws1', 'Codex', 'session-42', '/repo')).rejects.toThrow('Restore the Agent in its terminal');
    expect(invokeMock.mock.calls.length).toBe(calls);
  });
});

// §A 真正恢复测试 (Goal #3):
//  - 旧 OTP 过期但授权有效 → 断线恢复无需验证码
//  - 授权撤销 → 恢复被拒绝
//  - 不持久化 TOTP；优先 trust-grant；旧 host fallback 到 _verifiedCode
// 这些不变量覆盖 Goal 关心的「不能仅凭发出了 ping 宣称连接健康」——
// 重连的判定是 host 的 verifyTotp / tryTrustGrant 真实结果，**不**是
// provider.state 切到 'connected' 就完事。
describe('A — CloudRemoteConnection recovery: trust-grant 静默授权 + 撤销', () => {
  it('旧 OTP 过期但 trust-grant 有效 → 断线恢复无需重输 TOTP', async () => {
    // 模拟场景：TOTP 30s 窗口已滑过；但本次会话已通过 §7.4 trust 握手拿到
    // host 签发 token，重连后 cloudRemote 应走 tryTrustGrant 重新静默开门。
    const handle = fakeHandle();
    // 1) 没有 setVerifiedCode（用户从未在本次会话输过 TOTP — 旧 OTP 已过期）
    // 2) tryTrustGrant → true（trust 仍有效）
    (handle.tryTrustGrant as unknown as ReturnType<typeof vi.fn>).mockResolvedValue(true);
    const conn = new CloudRemoteConnection(handle as never, { invoke: vi.fn(), listen: vi.fn(async () => () => {}) } as never);
    await conn.init();
    let reconnected = 0;
    conn.onReconnect(() => reconnected++);
    conn.notifyState('disconnected');
    conn.notifyState('connected');
    await flush();
    // 关键断言：tryTrustGrant 走了；verifyTotp **没**被调（避免用户被骚扰输码）
    expect((handle.tryTrustGrant as unknown as ReturnType<typeof vi.fn>).mock.calls.length).toBeGreaterThanOrEqual(1);
    expect(verifyTotpSpy).not.toHaveBeenCalled();
    expect(reconnected).toBe(1);
    expect(conn.state()).toBe('connected');
  });

  it('trust-grant 也被撤销 → 恢复失败，进 \'error\' 终态，UI 走刷新拿新码', async () => {
    const handle = fakeHandle();
    (handle.tryTrustGrant as unknown as ReturnType<typeof vi.fn>).mockResolvedValue(false);
    const conn = new CloudRemoteConnection(handle as never, { invoke: vi.fn(), listen: vi.fn(async () => () => {}) } as never);
    await conn.init();
    let reconnected = 0;
    conn.onReconnect(() => reconnected++);
    conn.notifyState('disconnected');
    conn.notifyState('connected');
    await flush();
    expect(reconnected).toBe(0); // 没恢复，不触发 resync
    expect(conn.state()).toBe('error');
    // 错误分类为 'channel'（非用户/账户问题）→ UI 可提示「刷新拿新码」或重试
    expect((conn as any)._failure?.category).toBe('channel');
  });

  it('同时有 _verifiedCode 和 trust-grant → 优先 trust-grant（不重放过期验证码）', async () => {
    // Trust grant is the 24h fast path; cached code remains compatibility fallback.
    const handle = fakeHandle();
    (handle.tryTrustGrant as unknown as ReturnType<typeof vi.fn>).mockResolvedValue(true);
    verifyTotpSpy.mockResolvedValue(true);
    const conn = new CloudRemoteConnection(handle as never, { invoke: vi.fn(), listen: vi.fn(async () => () => {}) } as never);
    await conn.init();
    conn.setVerifiedCode('cached-code');
    conn.notifyState('disconnected');
    conn.notifyState('connected');
    await flush();
    expect((handle.tryTrustGrant as unknown as ReturnType<typeof vi.fn>).mock.calls.length).toBeGreaterThanOrEqual(1);
    expect(verifyTotpSpy).not.toHaveBeenCalled();
  });

  it('不允许 setVerifiedCode 后被后续 reconnect 清掉（不持久化是文档契约，内存保留 OK）', () => {
    const handle = fakeHandle();
    const conn = new CloudRemoteConnection(handle as never, { invoke: vi.fn(), listen: vi.fn(async () => () => {}) } as never);
    conn.setVerifiedCode('123456');
    expect((conn as any)._verifiedCode).toBe('123456');
    // 文档契约：cached code 仅在内存；不写入 localStorage / sessionStorage
    // （此为 cloudRemote 实现保证，测试仅校验 getVerifiedCode 暴露不写入
    // IndexedDB 之类 —— 用 jsdom 简单抽样 window.localStorage / sessionStorage）
    const storage = (globalThis as any).localStorage;
    if (storage && storage.getItem) {
      // 任何键里都不应出现 _verifiedCode / 123456
      for (let i = 0; i < storage.length; i++) {
        const k = storage.key(i);
        const v = k ? storage.getItem(k) : null;
        expect(v ?? '').not.toContain('123456');
        expect(k ?? '').not.toMatch(/_verifiedCode|setVerifiedCode/);
      }
    }
  });
});

describe('CloudRemoteConnection lifecycle', () => {
  it('ignores late provider state and errors after disconnect', async () => {
    const conn = await connected();
    let reconnected = 0;
    conn.onReconnect(() => reconnected++);
    conn.disconnect();
    verifyTotpSpy.mockClear();

    conn.notifyState('connected');
    conn.notifyError('late provider error', 'DEVICE_PARKED');
    await flush();

    expect(conn.state()).toBe('disconnected');
    expect(reconnected).toBe(0);
    expect(verifyTotpSpy).not.toHaveBeenCalled();
  });

  it('disconnect tears down listeners and the WebRTC handle', async () => {
    const conn = await connected();
    conn.subscribePane(PANE, { activationId: 1 });
    await flush();
    conn.disconnect();
    expect(conn.state()).toBe('disconnected');
    expect(disconnectSpy).toHaveBeenCalled();
    expect(handlers['pty-output-ws1-pane-a']).toBeUndefined();
    expect(handlers['pane-tree-changed']).toBeUndefined();
  });

  async function expectPaneRpcCancellation(
    stop: (conn: CloudRemoteConnection) => void | Promise<unknown>,
  ): Promise<void> {
    vi.useFakeTimers();
    const { injectedBridge, rpc, sent } = rpcBackedBridge();
    const conn = new CloudRemoteConnection(fakeHandle() as never, injectedBridge);
    try {
      conn.sendStdin(PANE, 'x', [{ ...PANE }, 'epoch', 1] as const);
      conn.refreshPane(PANE, 30, 100);
      await vi.advanceTimersByTimeAsync(40);
      expect(rpc.inFlight).toBe(2);

      await stop(conn);
      await Promise.resolve();
      await Promise.resolve();
      expect(rpc.inFlight).toBe(0);
      expect(sent.filter(
        (frame) => 'method' in frame && frame.method === '$/cancel',
      )).toHaveLength(2);

      const paneRequestsBefore = sent.filter(
        (frame) => 'method' in frame
          && (frame.method === 'write_terminal_input' || frame.method === 'resize_pane'),
      ).length;
      conn.sendStdin(PANE, 'after-destroy', [{ ...PANE }, 'epoch', 1] as const);
      conn.refreshPane(PANE, 40, 120);
      await vi.advanceTimersByTimeAsync(50);
      expect(sent.filter(
        (frame) => 'method' in frame
          && (frame.method === 'write_terminal_input' || frame.method === 'resize_pane'),
      )).toHaveLength(paneRequestsBefore);
      expect(rpc.inFlight).toBe(0);
    } finally {
      rpc.dispose();
      vi.useRealTimers();
    }
  }

  it('closePane aborts in-flight pane RPCs and rejects later sends', async () => {
    await expectPaneRpcCancellation(async (conn) => {
      expect(await conn.closePane(PANE)).toBe(true);
    });
  });

  it('pruneOutputs aborts in-flight pane RPCs and rejects later sends', async () => {
    await expectPaneRpcCancellation((conn) => conn.pruneOutputs(new Set()));
  });

  it('disconnect aborts in-flight pane RPCs and rejects later sends', async () => {
    await expectPaneRpcCancellation((conn) => conn.disconnect());
  });
});

describe('CloudRemoteConnection bounded parity guards', () => {
  it('keeps invalid pane input and resize requests local', async () => {
    const conn = await connected();
    expect(conn.sendStdin({ workspaceId: 'ws1', paneId: '' }, 'x', [{ ...{ workspaceId: 'ws1', paneId: '' } }, 'epoch', 1] as const)).toBe(false);
    expect(conn.sendStdin(PANE, '', [{ ...PANE }, 'epoch', 1] as const)).toBe(false);
    expect(conn.enqueueStdinTask({ workspaceId: 'ws1', paneId: '' }, () => 'x', [{ ...{ workspaceId: 'ws1', paneId: '' } }, 'epoch', 1] as const)).toBe(false);
    expect(conn.enqueueStdinTask(PANE, () => null, [{ ...PANE }, 'epoch', 1] as const)).toBe(true);
    conn.refreshPane(PANE, 0, 80);
    conn.refreshPane(PANE, 24, 0);
    expect(conn.lastRefreshSeq()).toBe(0);
    await flush();
    expect(invokeMock).not.toHaveBeenCalledWith('write_terminal_input', expect.anything(), expect.anything());
  });

  it('covers cloud parity commands and bounded history normalization', async () => {
    const conn = await connected();
    const capabilityOff = conn.onCapabilitiesChanged(vi.fn());
    capabilityOff();
    expect(conn.hasCapability('agent-messages')).toBe(true);

    await conn.markPaneAgent('ws1', 'pane-a', true, 'agent-42');
    await conn.markPaneAgent('ws1', 'pane-a', false);
    expect(await conn.listShells()).toEqual([]);
    await conn.changePaneShell('ws1', 'pane-a', {
      id: 'pwsh', label: 'PowerShell', program: 'pwsh', args: ['-NoLogo'],
    });
    expect(await conn.getTeammateTopology('ws1')).toBeUndefined();
    expect(await conn.listAgentHistory('ws1', 999)).toEqual([]);
    expect(await conn.listHitlPending()).toBeUndefined();
    expect(await conn.resolveHitlRemote('hitl-1', 'nonce-1', 'reject')).toBe('already-resolved');
    expect(await conn.getOrchestrationHealth()).toEqual({ suspendedAgents: 0, pendingHitl: 0 });
    expect(await conn.listSavedWorkspaceFiles()).toEqual([]);
    expect(await conn.openWorkspaceFromFile('missing.ridge')).toBeNull();
    expect(await conn.closeWorkspace('ws1')).toBe(true);
    expect(await conn.createWorkspace()).toBe('ws-new');

    expect(invokeMock).toHaveBeenCalledWith('register_teammate_agent', {
      workspaceId: 'ws1', paneId: 'pane-a', agentId: 'agent-42',
    }, expect.objectContaining({ signal: expect.any(AbortSignal) }));
    expect(invokeMock).toHaveBeenCalledWith('change_pane_shell', {
      paneId: 'pane-a', shell: 'pwsh', args: ['-NoLogo'],
    }, expect.objectContaining({ signal: expect.any(AbortSignal) }));
  });

  it('creates a pane for an empty workspace and does not hide close failures', async () => {
    const conn = await connected();
    let layoutCalls = 0;
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'get_pane_layout_for') {
        layoutCalls += 1;
        return layoutCalls === 1 ? null : {
          type: 'leaf', id: 'pane-created', title: 'created', cwd: '/tmp',
        };
      }
      if (cmd === 'create_workspace') return 'ws-created';
      if (cmd === 'close_pane') throw new Error('close rejected');
      return undefined;
    });

    expect(await conn.createPane()).toBe('pane-created');
    expect(await conn.closePane(PANE)).toBe(false);
    expect(await conn.closePane(PANE)).toBe(false);
    expect(layoutCalls).toBe(2);
  });

  it('keeps cloud parity operations fail-safe when optional commands fail', async () => {
    const conn = await connected();
    const stateListener = vi.fn();
    const offState = conn.onStateChange(stateListener);
    offState();
    conn.notifyState('connecting');
    conn.notifyError('device parked', 'DEVICE_PARKED');
    expect(conn.state()).toBe('error');
    expect(conn.lastFailure()).toEqual({ category: 'parked', code: 'DEVICE_PARKED', message: 'device parked' });

    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'list_workspaces') return undefined;
      if (cmd === 'get_active_workspace_id') throw new Error('active unavailable');
      if (cmd === 'send_agent_message') return { messageId: 'm', deliveryId: 'd' };
      if (cmd === 'resume_agent_session') return { paneId: 42 };
      if (cmd === 'list_saved_workspace_files') throw new Error('saved unavailable');
      if (cmd === 'open_workspace_from_file') throw 'open unavailable';
      if (cmd === 'close_workspace' || cmd === 'switch_workspace') throw new Error('mutation unavailable');
      if (cmd === 'create_workspace') return '';
      if (cmd === 'get_pane_layout_for') throw new Error('layout unavailable');
      if (cmd === 'get_theme_data') return { themes: [] };
      if (cmd === 'set_teammate_groups') return {groups:[],groupsRevision:4};
      return undefined;
    });

    await expect(conn.listWorkspaces()).resolves.toEqual({ workspaces: [] });
    await expect(conn.sendAgentMessage({ ...PANE, generation: 2, lease: 'lease' }, 'hello'))
      .resolves.toMatchObject({ messageId: 'm' });
    await expect(conn.setTeammateGroups('ws1', [], 3)).resolves.toEqual({groups:[],groupsRevision:4});
    expect(invokeMock).toHaveBeenCalledWith('set_teammate_groups',{workspaceId:'ws1',groups:[],expectedRevision:3});
    await expect(conn.resumeAgentSession('ws1', 'Codex', 's', '/repo')).rejects.toThrow('Restore the Agent in its terminal');
    await expect(conn.listSavedWorkspaceFiles()).resolves.toEqual([]);
    await expect(conn.openWorkspaceFromFile('missing.ridge')).rejects.toThrow('open unavailable');
    await expect(conn.closeWorkspace('ws1')).resolves.toBe(false);
    await expect(conn.switchWorkspace('ws2')).resolves.toBe(false);
    await expect(conn.createWorkspace('')).resolves.toBeNull();
    await expect(conn.listWorkspacePanes('ws1')).rejects.toThrow('layout unavailable');
    conn.cycleTheme('missing');
    await flush();
    expect(conn.getPaneOutput(PANE)).toEqual([]);
    conn.setHostClipboard('ignored');
    conn.connect();
    conn.send();
    conn.pruneOutputs(new Set());
  });
});
