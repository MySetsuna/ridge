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
import type { WsMessage } from './wsRemote';

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
      // §history-pull: seq-cursor scrollback. tail seeds the first screen; before
      // pages one batch older then reports at_oldest so paging stops.
      case 'get_pane_scrollback_tail':
        return { bytes: 'HIST', start_seq: 100, at_oldest: false, head_seq: 116 };
      case 'get_pane_scrollback_before':
        return { bytes: 'OLDER', start_seq: 60, at_oldest: true, head_seq: 116 };
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
    const metas: Array<[string, string | null, string | null]> = [];
    conn.onMessage((m) => msgs.push(m));
    conn.onMetadata((id, title, cwd) => metas.push([id, title, cwd]));

    conn.listPanes();
    await flush();

    const panesMsg = msgs.find((m) => m.type === 'panes');
    expect(panesMsg).toEqual({ type: 'panes', panes: [
      { id: 'pane-a', title: 'A', cwd: '/a' },
      { id: 'pane-b', title: undefined, cwd: undefined },
    ] });
    expect(metas).toContainEqual(['pane-a', 'A', '/a']);
    expect(metas).toContainEqual(['pane-b', null, null]);
  });

  it('subscribePane seeds a scrollback tail (RIS + history) then streams live pty bytes', async () => {
    const conn = await connected();
    const got: Array<[string, Uint8Array]> = [];
    conn.onRawBytes((id, bytes) => got.push([id, bytes]));

    conn.subscribePane('pane-a');
    await flush();

    // §history-pull: controller pulls its own ~1.5-screen tail first (no host dump)…
    expect(invokeMock).toHaveBeenCalledWith(
      'get_pane_scrollback_tail',
      expect.objectContaining({ paneId: 'pane-a' }),
    );
    // …emitted as the RIS'd reconcile first-frame (\x1bc + history).
    expect(got).toHaveLength(1);
    expect(got[0][0]).toBe('pane-a');
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

  it('fetchOlderScrollback pages one older batch, advances the cursor, then stops at oldest', async () => {
    const conn = await connected();
    conn.subscribePane('pane-a');
    await flush();

    // First scroll-up: pages older history (bytes < tail.start_seq) and returns it.
    const older = await conn.fetchOlderScrollback('pane-a');
    expect(invokeMock).toHaveBeenCalledWith(
      'get_pane_scrollback_before',
      expect.objectContaining({ paneId: 'pane-a', beforeSeq: 100 }),
    );
    expect(older && new TextDecoder().decode(older)).toBe('OLDER');

    // The mock's page reported at_oldest → a further scroll-up is a no-op (null),
    // and doesn't fire another before-fetch.
    const before = invokeMock.mock.calls.filter((c) => c[0] === 'get_pane_scrollback_before').length;
    expect(await conn.fetchOlderScrollback('pane-a')).toBeNull();
    const after = invokeMock.mock.calls.filter((c) => c[0] === 'get_pane_scrollback_before').length;
    expect(after).toBe(before); // stopped — no redundant fetch at oldest
  });

  it('fetchOlderScrollback returns null when the pane was never subscribed (no cursor)', async () => {
    const conn = await connected();
    expect(await conn.fetchOlderScrollback('pane-z')).toBeNull();
  });

  it('subscribePane is idempotent per pane', async () => {
    const conn = await connected();
    conn.subscribePane('pane-a');
    await flush();
    conn.subscribePane('pane-a');
    await flush();
    const regCalls = invokeMock.mock.calls.filter((c) => c[0] === 'register_pane_delta_channel');
    expect(regCalls).toHaveLength(1);
  });

  it('sendStdin writes to the pty', async () => {
    const conn = await connected();
    conn.sendStdin('pane-a', 'ls\n');
    await flush();
    expect(invokeMock).toHaveBeenCalledWith('write_to_pty', { paneId: 'pane-a', data: 'ls\n' });
  });

  it('claimPane resizes the host pty and bumps the refresh seq', async () => {
    const conn = await connected();
    const before = conn.lastRefreshSeq();
    conn.claimPane('pane-a', 30, 100, 0, 0);
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
    conn.subscribePane('pane-a');
    await flush();
    expect(handlers['pty-output-ws1-pane-a']).toBeTypeOf('function');

    const ok = await conn.closePane('pane-a');
    expect(ok).toBe(true);
    expect(invokeMock).toHaveBeenCalledWith('close_pane', { paneId: 'pane-a' });
    expect(handlers['pty-output-ws1-pane-a']).toBeUndefined();
  });

  it('pruneOutputs releases listeners for panes the host dropped', async () => {
    const conn = await connected();
    conn.subscribePane('pane-a');
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
    conn.subscribePane('pane-a');
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
    conn.subscribePane('pane-a');
    await flush();
    conn.disconnect();
    expect(conn.state()).toBe('disconnected');
    expect(disconnectSpy).toHaveBeenCalled();
    expect(handlers['pty-output-ws1-pane-a']).toBeUndefined();
    expect(handlers['pane-tree-changed']).toBeUndefined();
  });
});
