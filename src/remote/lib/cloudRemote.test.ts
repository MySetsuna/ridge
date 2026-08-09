import { describe, it, expect, vi, beforeEach } from 'vitest';

// ── Mock the tauriShim-aliased Tauri API (in the real mobile build these resolve
//    to the WebRTC bridge shims; here we drive them directly). vi.mock factories
//    are hoisted, so the mocks must be created via vi.hoisted to be referenceable. ──
const { invokeMock, listenMock, ChannelMock } = vi.hoisted(() => {
  class ChannelMock {
    onmessage: (v: unknown) => void = () => {};
  }
  return { invokeMock: vi.fn(), listenMock: vi.fn(), ChannelMock };
});

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  Channel: ChannelMock,
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: (...args: unknown[]) => listenMock(...args),
}));

import { CloudRemoteConnection } from './cloudRemote';
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
    adapter: {} as never,
    hostDevice: 'dev',
    verifyTotp: verifyTotpSpy,
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
        && frame.method !== 'write_to_pty'
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
});

describe('CloudRemoteConnection panes', () => {
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

  it('subscribePane seeds a scrollback tail (RIS + history) then streams live pty bytes', async () => {
    const conn = await connected();
    const got: Array<[PaneRef, Uint8Array]> = [];
    conn.onRawBytes((pane, bytes) => got.push([pane, bytes]));

    conn.subscribePane(PANE);
    await flush();

    // §R-CLOUD-CONVERGE: controller pulls ONE host-built resync frame (no client
    // assembly, no separate preamble invoke)…
    expect(invokeMock).toHaveBeenCalledWith(
      'get_pane_resync_frame',
      expect.objectContaining({ paneId: 'pane-a' }),
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
    // …and feeds `frame` verbatim as the first frame (RIS + preamble + history).
    expect(got).toHaveLength(1);
    expect(got[0][0]).toEqual(PANE);
    expect(new TextDecoder().decode(got[0][1])).toBe('\x1bcHIST');

    // Then the live stream is wired via register_pane_delta_channel (→ subscribe-pane).
    expect(invokeMock).toHaveBeenCalledWith(
      'register_pane_delta_channel',
      expect.objectContaining({ paneId: 'pane-a', workspaceId: 'ws1' }),
    );
    const evt = 'pty-output-ws1-pane-a';
    expect(handlers[evt]).toBeTypeOf('function');

    handlers[evt]({ payload: { data: 'hi' } });
    expect(got).toHaveLength(2);
    expect(new TextDecoder().decode(got[1][1])).toBe('hi');
  });

  it('keeps live bytes ordered while the bounded visual seed RPC is pending', async () => {
    const conn = await connected();
    const got: Uint8Array[] = [];
    conn.onRawBytes((_pane, bytes) => got.push(bytes));
    let releaseSeed!: (frame: { frame: string; start_seq: number; at_oldest: boolean; head_seq: number }) => void;
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'get_pane_resync_frame') {
        return new Promise((resolve) => { releaseSeed = resolve; });
      }
      return undefined;
    });

    conn.subscribePane(PANE);
    for (let i = 0; i < 20 && !releaseSeed; i += 1) await Promise.resolve();
    expect(handlers['pty-output-ws1-pane-a']).toBeTypeOf('function');
    expect(releaseSeed).toBeTypeOf('function');
    handlers['pty-output-ws1-pane-a']({ payload: { data: 'LIVE' } });
    expect(got.map((bytes) => new TextDecoder().decode(bytes))).toEqual(['LIVE']);

    releaseSeed({ frame: '\x1bcSEED', start_seq: 10, at_oldest: false, head_seq: 14 });
    await flush();
    expect(got.map((bytes) => new TextDecoder().decode(bytes))).toEqual(['LIVE', '\x1bcSEED', 'LIVE']);
  });

  it('requests host resync without replacing the live listener', async () => {
    const conn = await connected();
    conn.subscribePane(PANE);
    await flush();
    const listener = handlers['pty-output-ws1-pane-a'];
    expect(listener).toBeTypeOf('function');
    invokeMock.mockClear();

    conn.resyncPane(PANE);
    await flush();

    expect(invokeMock).toHaveBeenCalledWith(
      'resync_pane_raw',
      { paneId: PANE.paneId, workspaceId: PANE.workspaceId },
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
    expect(handlers['pty-output-ws1-pane-a']).toBe(listener);
  });

  it('§R-CLOUD-CONVERGE version-skew: falls back to RIS + tail when the host lacks get_pane_resync_frame', async () => {
    const conn = await connected();
    const got: Array<[PaneRef, Uint8Array]> = [];
    conn.onRawBytes((pane, bytes) => got.push([pane, bytes]));

    // Simulate an OLDER desktop host whose allow-list rejects the new command.
    invokeMock.mockImplementation(async (cmd: string, args?: unknown) => {
      if (cmd === 'get_pane_resync_frame') throw new Error('METHOD_NOT_FOUND');
      if (cmd === 'get_pane_scrollback_tail') return { bytes: 'HIST', start_seq: 100, at_oldest: false, head_seq: 116 };
      if (cmd === 'register_pane_delta_channel') return undefined;
      void args;
      return undefined;
    });

    conn.subscribePane(PANE);
    await flush();

    // The new command was attempted first, then the legacy tail seeded RIS + history
    // (no preamble — exactly the prior shipped cloud behavior, not a regression).
    expect(invokeMock).toHaveBeenCalledWith(
      'get_pane_resync_frame',
      expect.objectContaining({ paneId: 'pane-a' }),
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
    expect(invokeMock).toHaveBeenCalledWith(
      'get_pane_scrollback_tail',
      expect.objectContaining({ paneId: 'pane-a' }),
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
    expect(got).toHaveLength(1);
    expect(new TextDecoder().decode(got[0][1])).toBe('\x1bcHIST');
  });

  it('keeps the live subscription when both visual seed commands are unavailable', async () => {
    const conn = await connected();
    const got: string[] = [];
    conn.onRawBytes((_pane, bytes) => got.push(new TextDecoder().decode(bytes)));
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'get_pane_resync_frame' || cmd === 'get_pane_scrollback_tail') {
        throw new Error('seed command unavailable');
      }
      return undefined;
    });

    conn.subscribePane(PANE);
    await flush();
    handlers['pty-output-ws1-pane-a']({ payload: { data: 'LIVE' } });
    expect(got).toEqual(['LIVE']);
    expect(handlers['pty-output-ws1-pane-a']).toBeTypeOf('function');
  });

  it('retries when listener registration succeeds but host subscription fails', async () => {
    const conn = await connected();
    vi.useFakeTimers();
    let registrations = 0;
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'register_pane_delta_channel') {
        registrations += 1;
        if (registrations === 1) throw new Error('registration unavailable');
      }
      return undefined;
    });

    try {
      conn.subscribePane(PANE);
      await Promise.resolve();
      await Promise.resolve();
      expect(registrations).toBe(1);

      await vi.advanceTimersByTimeAsync(100);
      await Promise.resolve();
      await Promise.resolve();
      expect(registrations).toBe(2);
      expect(handlers['pty-output-ws1-pane-a']).toBeTypeOf('function');
    } finally {
      conn.disconnect();
      vi.useRealTimers();
    }
  });

  it('resume subscriptions skip the destructive visual seed', async () => {
    const conn = await connected();
    invokeMock.mockClear();

    conn.subscribePane(PANE, { resume: true });
    await flush();

    expect(invokeMock).not.toHaveBeenCalledWith('get_pane_resync_frame', expect.anything(), expect.anything());
    expect(invokeMock).not.toHaveBeenCalledWith('get_pane_scrollback_tail', expect.anything(), expect.anything());
    expect(handlers['pty-output-ws1-pane-a']).toBeTypeOf('function');
  });

  it('fetchOlderScrollback pages one older batch, advances the cursor, then stops at oldest', async () => {
    const conn = await connected();
    conn.subscribePane(PANE);
    await flush();

    // First scroll-up: pages older history (bytes < tail.start_seq) and returns it.
    const older = await conn.fetchOlderScrollback(PANE);
    expect(invokeMock).toHaveBeenCalledWith(
      'get_pane_scrollback_before',
      expect.objectContaining({ paneId: 'pane-a', beforeSeq: 100 }),
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
    expect(older && new TextDecoder().decode(older.bytes)).toBe('OLDER');
    expect(older?.commit()).toBe(true);

    // The mock's page reported at_oldest → a further scroll-up is a no-op (null),
    // and doesn't fire another before-fetch.
    const before = invokeMock.mock.calls.filter((c) => c[0] === 'get_pane_scrollback_before').length;
    expect(await conn.fetchOlderScrollback(PANE)).toBeNull();
    const after = invokeMock.mock.calls.filter((c) => c[0] === 'get_pane_scrollback_before').length;
    expect(after).toBe(before); // stopped — no redundant fetch at oldest
  });

  it('fetchOlderScrollback returns null when the pane was never subscribed (no cursor)', async () => {
    const conn = await connected();
    expect(await conn.fetchOlderScrollback({ workspaceId: 'ws1', paneId: 'pane-z' })).toBeNull();
  });

  it('rejects malformed older pages and records an empty oldest page', async () => {
    const conn = await connected();
    conn.subscribePane(PANE);
    await flush();
    let mode: 'malformed' | 'empty' = 'malformed';
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'get_pane_scrollback_before') {
        return mode === 'malformed'
          ? { bytes: 'BAD', start_seq: 100, end_seq: 100, at_oldest: false }
          : { bytes: '', start_seq: 0, end_seq: 100, at_oldest: true };
      }
      return undefined;
    });

    expect(await conn.fetchOlderScrollback(PANE)).toBeNull();
    mode = 'empty';
    expect(await conn.fetchOlderScrollback(PANE)).toBeNull();
    const calls = invokeMock.mock.calls.filter((call) => call[0] === 'get_pane_scrollback_before');
    expect(calls).toHaveLength(2);
    expect(await conn.fetchOlderScrollback(PANE)).toBeNull();
    expect(invokeMock.mock.calls.filter((call) => call[0] === 'get_pane_scrollback_before')).toHaveLength(2);
  });

  it('clears the older-page flight after a host rejection so a later fetch can recover', async () => {
    const conn = await connected();
    conn.subscribePane(PANE);
    await flush();
    let attempts = 0;
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'get_pane_scrollback_before') {
        attempts += 1;
        if (attempts === 1) throw new Error('history temporarily unavailable');
        return { bytes: 'OLDER', start_seq: 60, end_seq: 100, at_oldest: false };
      }
      return undefined;
    });

    expect(await conn.fetchOlderScrollback(PANE)).toBeNull();
    const page = await conn.fetchOlderScrollback(PANE);
    expect(page && new TextDecoder().decode(page.bytes)).toBe('OLDER');
    expect(page?.discard()).toBeUndefined();
    expect(attempts).toBe(2);
  });

  it('subscribePane is idempotent per pane', async () => {
    const conn = await connected();
    conn.subscribePane(PANE);
    await flush();
    conn.subscribePane(PANE);
    await flush();
    const regCalls = invokeMock.mock.calls.filter((c) => c[0] === 'register_pane_delta_channel');
    expect(regCalls).toHaveLength(1);
  });

  it('retries a transient subscribe failure with bounded backoff', async () => {
    const conn = await connected();
    vi.useFakeTimers();
    let attempts = 0;
    listenMock.mockImplementation(async (name: string, handler: (e: { payload: unknown }) => void) => {
      if (name === 'pty-output-ws1-pane-a') {
        attempts += 1;
        if (attempts === 1) throw new Error('transient subscribe failure');
      }
      handlers[name] = handler;
      return () => { delete handlers[name]; };
    });
    try {
      conn.subscribePane(PANE);
      await Promise.resolve();
      await Promise.resolve();
      expect(attempts).toBe(1);

      await vi.advanceTimersByTimeAsync(99);
      expect(attempts).toBe(1);
      await vi.advanceTimersByTimeAsync(1);
      await Promise.resolve();
      await Promise.resolve();

      expect(attempts).toBe(2);
      expect(handlers['pty-output-ws1-pane-a']).toBeTypeOf('function');
    } finally {
      conn.disconnect();
      vi.useRealTimers();
    }
  });

  it('stops subscribe retries after the bounded attempt cap', async () => {
    const conn = await connected();
    vi.useFakeTimers();
    let attempts = 0;
    listenMock.mockImplementation(async (name: string) => {
      if (name === 'pty-output-ws1-pane-a') {
        attempts += 1;
        throw new Error('persistent subscribe failure');
      }
      return () => {};
    });
    try {
      conn.subscribePane(PANE);
      await Promise.resolve();
      await Promise.resolve();
      for (const delay of [100, 200, 400, 800]) {
        await vi.advanceTimersByTimeAsync(delay);
        await Promise.resolve();
        await Promise.resolve();
      }
      expect(attempts).toBe(5); // initial attempt + four retries
      await vi.advanceTimersByTimeAsync(5_000);
      expect(attempts).toBe(5);
    } finally {
      conn.disconnect();
      vi.useRealTimers();
    }
  });

  it('sendStdin writes to the pty', async () => {
    const conn = await connected();
    conn.sendStdin(PANE, 'ls\n');
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(invokeMock).toHaveBeenCalledWith('write_to_pty', {
      workspaceId: 'ws1',
      paneId: 'pane-a',
      data: 'ls\n',
      inputSourceId: expect.any(String),
      inputSequence: 1,
    }, expect.objectContaining({ signal: expect.any(AbortSignal) }));
  });

  it('sends the first key immediately and coalesces later input without reordering', async () => {
    const conn = await connected();
    invokeMock.mockClear();
    let releaseFirst!: () => void;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd !== 'write_to_pty') return Promise.resolve(undefined);
      if (!releaseFirst) return new Promise<void>((resolve) => { releaseFirst = resolve; });
      return Promise.resolve(undefined);
    });

    conn.sendStdin(PANE, 'a');
    // Zero input window: the first byte is admitted in the same turn.
    expect(invokeMock.mock.calls.filter((call) => call[0] === 'write_to_pty')).toHaveLength(1);
    conn.sendStdin(PANE, 'b');
    await new Promise((resolve) => setTimeout(resolve, 10));
    conn.sendStdin(PANE, 'c');
    conn.sendStdin(PANE, 'd');
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(invokeMock.mock.calls.filter((call) => call[0] === 'write_to_pty')).toHaveLength(1);

    releaseFirst();
    await new Promise((resolve) => setTimeout(resolve, 10));
    const writes = invokeMock.mock.calls.filter((call) => call[0] === 'write_to_pty');
    expect(writes).toHaveLength(2);
    expect(writes[0][1]).toMatchObject({ data: 'a', inputSequence: 1 });
    expect(writes[1][1]).toMatchObject({
      data: 'bcd',
      inputSourceId: writes[0][1].inputSourceId,
      inputSequence: 2,
    });
  });

  it('keeps a 1,000-event input burst bounded after the immediate first RPC', async () => {
    const conn = await connected();
    invokeMock.mockClear();
    vi.useFakeTimers();
    try {
      for (let i = 0; i < 1_000; i += 1) conn.sendStdin(PANE, 'x');

      expect(invokeMock.mock.calls.filter((call) => call[0] === 'write_to_pty')).toHaveLength(1);
      await vi.runAllTicks();

      const writes = invokeMock.mock.calls.filter((call) => call[0] === 'write_to_pty');
      expect(writes).toHaveLength(2);
      expect(writes[0][1]).toMatchObject({
        data: 'x',
        inputSequence: 1,
      });
      expect(writes[1][1]).toMatchObject({
        data: 'x'.repeat(999),
        inputSequence: 2,
      });
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
      if (cmd !== 'write_to_pty') return Promise.resolve(undefined);
      attempts += 1;
      return attempts === 1
        ? Promise.reject(new RpcTimeoutError('write_to_pty', 1_000))
        : Promise.resolve(undefined);
    });
    vi.useFakeTimers();
    try {
      conn.sendStdin(PANE, 'x');
      // The first request is immediate; let its rejection schedule the 100 ms
      // exponential-backoff timer before advancing the fake clock.
      await Promise.resolve();
      await Promise.resolve();
      expect(attempts).toBe(1);
      await vi.advanceTimersByTimeAsync(99);
      expect(attempts).toBe(1);
      await vi.advanceTimersByTimeAsync(1);
      expect(attempts).toBe(2);
      const writes = invokeMock.mock.calls.filter((call) => call[0] === 'write_to_pty');
      expect(writes[0][1]).toMatchObject({ data: 'x', inputSequence: 1 });
      expect(writes[1][1]).toMatchObject({
        data: 'x',
        inputSourceId: writes[0][1].inputSourceId,
        inputSequence: 1,
      });
    } finally {
      vi.useRealTimers();
    }
  });

  it('pauses input retries after five consecutive timeouts until explicit pane recovery', async () => {
    const conn = await connected();
    invokeMock.mockClear();
    let attempts = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd !== 'write_to_pty') return Promise.resolve(undefined);
      attempts += 1;
      return Promise.reject(new RpcTimeoutError('write_to_pty', 1_000));
    });
    vi.useFakeTimers();
    try {
      conn.sendStdin(PANE, 'x');
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

      conn.claimPane(PANE, 24, 80);
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
      conn.claimPane(PANE, 30, 100, 0, 0);
      await vi.advanceTimersByTimeAsync(40);
      expect(invokeMock).toHaveBeenCalledWith('resize_pane', {
        workspaceId: 'ws1', paneId: 'pane-a', rows: 30, cols: 100,
      }, expect.objectContaining({ signal: expect.any(AbortSignal) }));
      expect(conn.lastRefreshSeq()).toBe(before + 1);
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
      expect(invokeMock.mock.calls.filter((c) => c[0] === 'resize_pane')).toHaveLength(2);
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
    conn.subscribePane(PANE);
    await flush();
    expect(handlers['pty-output-ws1-pane-a']).toBeTypeOf('function');

    const ok = await conn.closePane(PANE);
    expect(ok).toBe(true);
    expect(invokeMock).toHaveBeenCalledWith('close_pane', { workspaceId: 'ws1', paneId: 'pane-a' });
    expect(handlers['pty-output-ws1-pane-a']).toBeUndefined();
  });

  it('pruneOutputs releases listeners for panes the host dropped', async () => {
    const conn = await connected();
    conn.subscribePane(PANE);
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
    conn.subscribePane({ workspaceId: 'ws2', paneId: 'pane-a' });
    await flush();
    expect(handlers['pty-output-ws2-pane-a']).toBeTypeOf('function');
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
    expect(verifyTotpSpy).toHaveBeenCalledWith('123456'); // re-auth with cached code
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
      conn.sendStdin(PANE, 'kept');
      await vi.advanceTimersByTimeAsync(4);
      const first = sent.find(
        (frame) => 'method' in frame && frame.method === 'write_to_pty',
      );
      expect(first).toBeDefined();

      conn.notifyState('disconnected');
      conn.notifyState('connected');
      for (let i = 0; i < 8; i += 1) await Promise.resolve();

      const writes = sent.filter(
        (frame) => 'method' in frame && frame.method === 'write_to_pty',
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

  it('resumes an Agent session through host structured launch with recorded CWD', async () => {
    const conn = await connected();
    await expect(conn.resumeAgentSession('ws1', 'Codex', 'session-42', 'C:\\repo\\shared'))
      .resolves.toBe('pane-resumed');
    expect(invokeMock).toHaveBeenCalledWith('resume_agent_session', {
      workspaceId: 'ws1',
      agent: 'Codex',
      sessionId: 'session-42',
      cwd: 'C:\\repo\\shared',
    });
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
    conn.subscribePane(PANE);
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
      conn.sendStdin(PANE, 'x');
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
          && (frame.method === 'write_to_pty' || frame.method === 'resize_pane'),
      ).length;
      conn.sendStdin(PANE, 'after-destroy');
      conn.refreshPane(PANE, 40, 120);
      await vi.advanceTimersByTimeAsync(50);
      expect(sent.filter(
        (frame) => 'method' in frame
          && (frame.method === 'write_to_pty' || frame.method === 'resize_pane'),
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
    expect(conn.sendStdin({ workspaceId: 'ws1', paneId: '' }, 'x')).toBe(false);
    expect(conn.sendStdin(PANE, '')).toBe(false);
    expect(conn.enqueueStdinTask({ workspaceId: 'ws1', paneId: '' }, () => 'x')).toBe(false);
    expect(conn.enqueueStdinTask(PANE, () => null)).toBe(true);
    conn.refreshPane(PANE, 0, 80);
    conn.refreshPane(PANE, 24, 0);
    expect(conn.lastRefreshSeq()).toBe(0);
    await flush();
    expect(invokeMock).not.toHaveBeenCalledWith('write_to_pty', expect.anything(), expect.anything());
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
    expect(await conn.listAgentHistory(999)).toEqual([]);
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
});
