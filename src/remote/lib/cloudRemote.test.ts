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
import type { PaneRef, WsMessage } from '@ridge/remote';

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
    { type: 'leaf', id: 'pane-a', title: 'A', cwd: '/a' },
    { type: 'leaf', id: 'pane-b' },
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

/** Flush pending microtasks/macrotasks so fire-and-forget async settles. */
const flush = () => new Promise((r) => setTimeout(r, 0));

beforeEach(() => {
  handlers = {};
  invokeMock.mockReset();
  listenMock.mockReset();
  invokeMock.mockImplementation(async (cmd: string) => {
    switch (cmd) {
      case 'get_active_workspace_id': return 'ws1';
      case 'get_pane_layout': return LAYOUT;
      case 'get_pane_layout_for': return LAYOUT;
      case 'list_workspaces': return [{ id: 'ws1', name: 'One' }, { id: 'ws2', name: 'Two' }];
      case 'split_pane': return { pane_id: 'pane-new', initial_cwd: null };
      case 'create_workspace': return 'ws-new';
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
    // iter-61：叶子附带 agent 标记态（agent_state==='busy'），供工作区弹层的标记按钮。
    expect(panesMsg).toEqual({ type: 'panes', workspaceId: 'ws1', panes: [
      { id: 'pane-a', title: 'A', cwd: '/a', isAgent: false },
      { id: 'pane-b', title: undefined, cwd: undefined, isAgent: false },
    ] });
    expect(metas).toContainEqual([{ workspaceId: 'ws1', paneId: 'pane-a' }, 'A', '/a']);
    expect(metas).toContainEqual([{ workspaceId: 'ws1', paneId: 'pane-b' }, null, null]);
  });

  it('keeps a delayed pane snapshot scoped to the workspace that requested it', async () => {
    const conn = await connected();
    const msgs: WsMessage[] = [];
    let resolveLayout!: (layout: PaneNode) => void;
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'get_pane_layout') {
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
    expect(invokeMock).toHaveBeenCalledWith('get_pane_resync_frame', expect.objectContaining({ paneId: 'pane-a' }));
    expect(invokeMock).toHaveBeenCalledWith('get_pane_scrollback_tail', expect.objectContaining({ paneId: 'pane-a' }));
    expect(got).toHaveLength(1);
    expect(new TextDecoder().decode(got[0][1])).toBe('\x1bcHIST');
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

  it('subscribePane is idempotent per pane', async () => {
    const conn = await connected();
    conn.subscribePane(PANE);
    await flush();
    conn.subscribePane(PANE);
    await flush();
    const regCalls = invokeMock.mock.calls.filter((c) => c[0] === 'register_pane_delta_channel');
    expect(regCalls).toHaveLength(1);
  });

  it('sendStdin writes to the pty', async () => {
    const conn = await connected();
    conn.sendStdin(PANE, 'ls\n');
    await flush();
    expect(invokeMock).toHaveBeenCalledWith('write_to_pty', { workspaceId: 'ws1', paneId: 'pane-a', data: 'ls\n' });
  });

  it('claimPane resizes the host pty and bumps the refresh seq', async () => {
    const conn = await connected();
    const before = conn.lastRefreshSeq();
    conn.claimPane(PANE, 30, 100, 0, 0);
    await flush();
    expect(invokeMock).toHaveBeenCalledWith('resize_pane', {
      workspaceId: 'ws1', paneId: 'pane-a', rows: 30, cols: 100,
    });
    expect(conn.lastRefreshSeq()).toBe(before + 1);
  });

  it('createPane splits the first existing leaf', async () => {
    const conn = await connected();
    const id = await conn.createPane();
    expect(invokeMock).toHaveBeenCalledWith('split_pane', { paneId: 'pane-a', direction: 'horizontal' });
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
});

describe('CloudRemoteConnection lifecycle', () => {
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
});
