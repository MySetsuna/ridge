import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  FIRST_CONNECT_TIMEOUT_MS,
  RemoteConnection,
  SHELL_DISCOVERY_TIMEOUT_MS,
  type PaneRef,
} from './wsRemote';

type Handler<T extends (...args: never[]) => void> = T | null;

class FakeWebSocket {
  static readonly OPEN = 1;
  static readonly CLOSED = 3;
  static latest: FakeWebSocket | null = null;

  readonly sent: Record<string, unknown>[] = [];
  readonly url: string;
  readyState = FakeWebSocket.OPEN;
  binaryType = '';
  onopen: Handler<() => void> = null;
  onclose: Handler<(event: { code: number }) => void> = null;
  onerror: Handler<() => void> = null;
  onmessage: Handler<(event: MessageEvent) => void> = null;

  constructor(url: string) {
    this.url = url;
    FakeWebSocket.latest = this;
  }

  send(raw: string): void {
    if (this.readyState !== FakeWebSocket.OPEN) throw new Error('socket is closed');
    this.sent.push(JSON.parse(raw) as Record<string, unknown>);
  }

  open(): void {
    this.onopen?.();
  }

  receive(message: Record<string, unknown>): void {
    this.onmessage?.({ data: JSON.stringify(message) } as MessageEvent);
  }

  receiveBinary(bytes: Uint8Array): void {
    this.onmessage?.({ data: bytes.buffer } as MessageEvent);
  }

  close(): void {
    this.readyState = FakeWebSocket.CLOSED;
    this.onclose?.({ code: 1000 });
  }
}

const pane: PaneRef = { workspaceId: 'workspace-a', paneId: 'pane-a' };

function connect(): { conn: RemoteConnection; ws: FakeWebSocket } {
  const conn = new RemoteConnection();
  conn.connect('127.0.0.1', 9527, 'test-token', 'token', false);
  const ws = FakeWebSocket.latest;
  if (!ws) throw new Error('fake websocket was not constructed');
  ws.open();
  return { conn, ws };
}

function invokeFrames(ws: FakeWebSocket, method: string): Record<string, unknown>[] {
  return ws.sent.filter((frame) => frame.type === 'invoke-request' && frame.cmd === method);
}

function resolveInvoke(ws: FakeWebSocket, frame: Record<string, unknown>, result: unknown = null): void {
  ws.receive({ type: 'invoke-result', _reqId: frame._reqId, _result: result });
}

async function flushPromises(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

describe('RemoteConnection LAN pane RPC scheduler', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.stubGlobal('WebSocket', FakeWebSocket);
    vi.stubGlobal('location', { protocol: 'http:' });
    vi.spyOn(console, 'log').mockImplementation(() => {});
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    FakeWebSocket.latest = null;
    vi.useRealTimers();
  });

  it('turns a never-opened first connection into an actionable error', async () => {
    const conn = new RemoteConnection();
    const states: string[] = [];
    conn.onStateChange((state) => states.push(state));

    conn.connect('127.0.0.1', 9527, 'test-token', 'token', false);
    await vi.advanceTimersByTimeAsync(FIRST_CONNECT_TIMEOUT_MS);

    expect(conn.state()).toBe('error');
    expect(conn.lastFailure()).toEqual(expect.objectContaining({
      category: 'channel',
      message: 'initial remote connection timed out',
    }));
    expect(states).toContain('error');
    expect(FakeWebSocket.latest?.readyState).toBe(FakeWebSocket.CLOSED);
  });

  it('allows slow shell discovery to outlive the generic RPC timeout', async () => {
    const { conn, ws } = connect();
    const promise = conn.listShells();
    const frame = invokeFrames(ws, 'detect_available_shells')[0];
    let settled = false;
    void promise.then(() => { settled = true; }, () => { settled = true; });

    await vi.advanceTimersByTimeAsync(5_001);
    expect(settled).toBe(false);
    expect(SHELL_DISCOVERY_TIMEOUT_MS).toBeGreaterThan(5_001);

    resolveInvoke(ws, frame, [{
      id: 'powershell',
      label: 'Windows PowerShell 5.1',
      program: 'powershell.exe',
      args: [],
    }]);
    await expect(promise).resolves.toEqual([expect.objectContaining({ id: 'powershell' })]);
    conn.disconnect();
  });

  it('applies headless host capability hints before mounting optional panels', async () => {
    const { conn, ws } = connect();
    const changes: boolean[] = [];
    conn.onCapabilitiesChanged(() => changes.push(true));

    const promise = conn.listWorkspaces();
    const frame = ws.sent.find((item) => item.type === 'list-workspaces');
    if (!frame) throw new Error('workspace request was not sent');
    ws.receive({
      type: 'workspaces',
      _reqId: frame._reqId,
      workspaces: [{
        id: 'workspace-a',
        name: 'Ridge',
        active: true,
        capabilities: ['pane', 'fs', 'search', 'workspace'],
      }],
    });

    await expect(promise).resolves.toEqual({
      workspaces: [expect.objectContaining({ id: 'workspace-a' })],
    });
    expect(conn.hasCapability('pane')).toBe(true);
    expect(conn.hasCapability('teammate')).toBe(false);
    expect(conn.hasCapability('git')).toBe(false);
    expect(changes).toHaveLength(2);
    conn.disconnect();
  });

  it('applies kernel capability hints from the initial hello frame', () => {
    const { conn, ws } = connect();
    ws.receive({
      type: 'hello',
      version: 1,
      protocol: 'ridge-remote-ws',
      capabilities: ['pane', 'fs', 'search', 'workspace'],
    });
    expect(conn.hasCapability('pane')).toBe(true);
    expect(conn.hasCapability('teammate')).toBe(false);
    conn.disconnect();
  });

  it('breaks an unsupported teammate capability on a legacy host', async () => {
    const { conn, ws } = connect();
    const promise = conn.getTeammateTopology('workspace-a');
    const frame = invokeFrames(ws, 'get_teammate_topology')[0];
    ws.receive({
      type: 'invoke-result',
      _reqId: frame._reqId,
      _error: 'method not supported by kernel host: get_teammate_topology',
    });

    await expect(promise).rejects.toThrow('method not supported by kernel host');
    expect(conn.hasCapability('teammate')).toBe(false);
    conn.disconnect();
  });

  it('refuses ambiguous legacy raw frames and routes after one composite ref remains', async () => {
    const { conn, ws } = connect();
    const paneId = '01234567-89ab-cdef-0123-456789abcdef';
    const first = { workspaceId: 'workspace-a', paneId };
    const second = { workspaceId: 'workspace-b', paneId };
    const received: string[] = [];
    conn.onRawBytes((pane) => received.push(`${pane.workspaceId}:${pane.paneId}`));
    conn.subscribePane(first);
    conn.subscribePane(second);

    const frame = (payload: number) => {
      const bytes = new Uint8Array(17);
      const hex = paneId.replaceAll('-', '');
      for (let i = 0; i < 16; i++) bytes[i] = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16);
      bytes[16] = payload;
      return bytes;
    };

    ws.receiveBinary(frame(1));
    expect(received).toEqual([]);

    const closing = conn.closePane(first);
    ws.receive({ type: 'close-pane-result', success: true });
    await closing;
    ws.receiveBinary(frame(2));
    expect(received).toEqual(['workspace-b:' + paneId]);
    conn.disconnect();
  });

  it('shares legacy list-workspaces requests instead of overwriting the first pending', async () => {
    const { conn, ws } = connect();

    const first = conn.listWorkspaces();
    const second = conn.listWorkspaces();
    expect(ws.sent.filter((frame) => frame.type === 'list-workspaces')).toHaveLength(1);

    ws.receive({ type: 'workspaces', workspaces: [] });
    await expect(first).resolves.toEqual({ workspaces: [] });
    await expect(second).resolves.toEqual({ workspaces: [] });
    conn.disconnect();
  });

  it('releases the legacy coalescing slot after timeout', async () => {
    const { conn, ws } = connect();

    const first = conn.listWorkspaces();
    const firstRejected = expect(first).rejects.toThrow('WS request workspaces timed out');
    await vi.advanceTimersByTimeAsync(5_000);
    await firstRejected;

    const second = conn.listWorkspaces();
    expect(ws.sent.filter((frame) => frame.type === 'list-workspaces')).toHaveLength(2);
    ws.receive({ type: 'workspaces', workspaces: [] });
    await expect(second).resolves.toEqual({ workspaces: [] });
    conn.disconnect();
  });

  it('correlates concurrent scrollback pages by request id across panes', async () => {
    const { conn, ws } = connect();
    const paneB = { workspaceId: 'workspace-b', paneId: 'pane-b' };
    conn.subscribePane(pane, { active: false });
    conn.subscribePane(paneB, { active: false });
    ws.receive({ type: 'scrollback-meta', workspaceId: pane.workspaceId, paneId: pane.paneId, startSeq: 10, atOldest: false });
    ws.receive({ type: 'scrollback-meta', workspaceId: paneB.workspaceId, paneId: paneB.paneId, startSeq: 20, atOldest: false });

    const first = conn.fetchOlderScrollback(pane);
    const second = conn.fetchOlderScrollback(paneB);
    const pages = ws.sent.filter((frame) => frame.type === 'scrollback-before');
    expect(pages).toHaveLength(2);
    expect(pages[0]._reqId).not.toBe(pages[1]._reqId);

    ws.receive({ type: 'scrollback-before-result', _reqId: pages[1]._reqId, bytes: 'b', startSeq: 11, endSeq: 20, atOldest: true });
    ws.receive({ type: 'scrollback-before-result', _reqId: pages[0]._reqId, bytes: 'a', startSeq: 1, endSeq: 10, atOldest: true });
    const pageA = await first;
    const pageB = await second;
    expect(pageA?.bytes).toEqual(new TextEncoder().encode('a'));
    expect(pageB?.bytes).toEqual(new TextEncoder().encode('b'));
    expect(pageA?.commit()).toBe(true);
    expect(pageB?.commit()).toBe(true);
    conn.disconnect();
  });

  it('serializes different legacy payloads instead of sharing the wrong reply', async () => {
    const { conn, ws } = connect();

    const first = conn.listWorkspacePanes('workspace-a');
    const second = conn.listWorkspacePanes('workspace-b');
    const requests = () => ws.sent.filter((frame) => frame.type === 'list-workspace-panes');
    expect(requests()).toHaveLength(1);

    ws.receive({ type: 'workspace-panes', workspaceId: 'workspace-a', panes: [] });
    await expect(first).resolves.toEqual([]);
    await Promise.resolve();
    expect(requests()).toHaveLength(2);
    ws.receive({ type: 'workspace-panes', workspaceId: 'workspace-b', panes: [] });
    await expect(second).resolves.toEqual([]);
    conn.disconnect();
  });

  it('ignores a late workspace reply for another workspace', async () => {
    const { conn, ws } = connect();

    const first = conn.listWorkspacePanes('workspace-a');
    const second = conn.listWorkspacePanes('workspace-b');
    const requests = () => ws.sent.filter((frame) => frame.type === 'list-workspace-panes');
    expect(requests()).toHaveLength(1);

    // A delayed response from a previous workspace must not settle the
    // current legacy slot. The second request remains queued until A settles.
    ws.receive({ type: 'workspace-panes', workspaceId: 'workspace-b', panes: [{ id: 'wrong' }] });
    await flushPromises();
    expect(requests()).toHaveLength(1);

    ws.receive({ type: 'workspace-panes', workspaceId: 'workspace-a', panes: [{ id: 'pane-a' }] });
    await expect(first).resolves.toEqual([{ id: 'pane-a' }]);
    await flushPromises();
    expect(requests()).toHaveLength(2);

    ws.receive({ type: 'workspace-panes', workspaceId: 'workspace-b', panes: [{ id: 'pane-b' }] });
    await expect(second).resolves.toEqual([{ id: 'pane-b' }]);
    conn.disconnect();
  });

  it('keeps input ordered behind one acknowledged invoke request', async () => {
    const { conn, ws } = connect();

    expect(conn.sendStdin(pane, 'a')).toBe(true);
    expect(conn.sendStdin(pane, 'b')).toBe(true);
    expect(conn.sendStdin(pane, 'c')).toBe(true);

    const writes = invokeFrames(ws, 'write_to_pty');
    expect(writes).toHaveLength(1);
    expect(writes[0]).toMatchObject({
      args: { workspaceId: pane.workspaceId, paneId: pane.paneId, data: 'a' },
    });

    resolveInvoke(ws, writes[0]);
    await flushPromises();
    await flushPromises();
    const retried = invokeFrames(ws, 'write_to_pty');
    expect(retried).toHaveLength(2);
    expect(retried[1]).toMatchObject({
      args: { workspaceId: pane.workspaceId, paneId: pane.paneId, data: 'bc' },
    });

    resolveInvoke(ws, retried[1]);
    await flushPromises();
    expect(conn.rpcSchedulingDiagnostics).toMatchObject({
      inputCalls: 3,
      inputRequests: 2,
      inputBytesAccepted: 3,
      inputBytesCompleted: 3,
      queuedInputBytes: 0,
    });
    conn.disconnect();
  });

  it('keeps only the latest resize while one request is in flight', async () => {
    const { conn, ws } = connect();

    conn.refreshPane(pane, 24, 80, 0, 0);
    conn.refreshPane(pane, 30, 100, 0, 0);
    conn.refreshPane(pane, 40, 120, 0, 0);
    await vi.advanceTimersByTimeAsync(40);

    const first = invokeFrames(ws, 'resize_pane');
    expect(first).toHaveLength(1);
    expect(first[0]).toMatchObject({
      args: { workspaceId: pane.workspaceId, paneId: pane.paneId, rows: 40, cols: 120 },
    });

    conn.refreshPane(pane, 50, 140, 0, 0);
    resolveInvoke(ws, first[0]);
    await flushPromises();
    await vi.advanceTimersByTimeAsync(40);

    const all = invokeFrames(ws, 'resize_pane');
    expect(all).toHaveLength(2);
    expect(all[1]).toMatchObject({ args: { rows: 50, cols: 140 } });
    resolveInvoke(ws, all[1]);
    await flushPromises();
    expect(conn.rpcSchedulingDiagnostics).toMatchObject({
      resizeCalls: 4,
      resizeRequests: 2,
    });
    conn.disconnect();
  });

  it('turns an invoke timeout into bounded backoff and retry', async () => {
    const { conn, ws } = connect();

    conn.sendStdin(pane, 'retry-me');
    expect(invokeFrames(ws, 'write_to_pty')).toHaveLength(1);
    const first = invokeFrames(ws, 'write_to_pty')[0];

    await vi.advanceTimersByTimeAsync(5_000);
    expect(ws.sent).toContainEqual({ type: 'invoke-cancel', _reqId: first._reqId });
    expect(conn.rpcSchedulingDiagnostics).toMatchObject({
      inputFailures: 1,
      timeoutFailures: 1,
      retries: 1,
    });
    expect(invokeFrames(ws, 'write_to_pty')).toHaveLength(1);

    await vi.advanceTimersByTimeAsync(100);
    const retry = invokeFrames(ws, 'write_to_pty');
    expect(retry).toHaveLength(2);
    expect(retry[1].args).toEqual(retry[0].args);
    resolveInvoke(ws, retry[1]);
    await flushPromises();
    expect(conn.rpcSchedulingDiagnostics.queuedInputBytes).toBe(0);
    conn.disconnect();
  });

  it('retires pending pane work on prune without resurrecting a request', async () => {
    const { conn, ws } = connect();

    conn.sendStdin(pane, 'stale');
    expect(invokeFrames(ws, 'write_to_pty')).toHaveLength(1);
    conn.pruneOutputs(new Set());
    await flushPromises();
    await vi.advanceTimersByTimeAsync(10_000);

    expect(invokeFrames(ws, 'write_to_pty')).toHaveLength(1);
    expect(conn.rpcSchedulingDiagnostics.queuedInputBytes).toBe(0);
    conn.disconnect();
  });

  it('cancels pending pane RPCs before LAN pane destruction', async () => {
    const { conn, ws } = connect();

    conn.sendStdin(pane, 'stale-before-close');
    expect(invokeFrames(ws, 'write_to_pty')).toHaveLength(1);

    const closing = conn.closePane(pane);
    const closeFrame = ws.sent.find((frame) => frame.type === 'close-pane');
    expect(closeFrame).toBeDefined();

    // Resolving the close command must not allow the retired write lane to
    // retry after the PTY has gone away.
    ws.receive({ type: 'close-pane-result', success: true });
    await closing;
    await flushPromises();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(invokeFrames(ws, 'write_to_pty')).toHaveLength(1);
    expect(conn.rpcSchedulingDiagnostics.queuedInputBytes).toBe(0);
    conn.disconnect();
  });

  it('covers control-plane event listeners and teammate/workspace RPC projections', async () => {
    const { conn, ws } = connect();
    const messages: unknown[] = [];
    const metas: unknown[] = [];
    const resizes: unknown[] = [];
    const themes: unknown[] = [];
    conn.onMessage((message) => messages.push(message));
    conn.onMetadata((...args) => metas.push(args));
    conn.onPtyResize((...args) => resizes.push(args));
    conn.onTheme((...args) => themes.push(args));
    ws.receive({ type: 'output', workspaceId: pane.workspaceId, paneId: pane.paneId, data: 'one\ntwo' });
    expect(conn.getPaneOutput(pane)).toEqual(['one', 'two']);
    ws.receive({ type: 'pty-meta', workspaceId: pane.workspaceId, paneId: pane.paneId, title: 'Shell', cwd: '/repo' });
    ws.receive({ type: 'pty-resized', workspaceId: pane.workspaceId, paneId: pane.paneId, rows: 24, cols: 80 });
    ws.receive({ type: 'theme', id: 'dark', themeType: 'dark', colors: { background: '#000' } });
    expect(metas).toHaveLength(1);
    expect(resizes).toHaveLength(1);
    expect(themes).toEqual([[{ background: '#000' }, 'dark']]);
    expect(conn.lastTheme()).toMatchObject({ id: 'dark' });

    const resolveInvoke = async <T>(
      action: () => Promise<T>,
      result: unknown,
      method?: string,
    ): Promise<T> => {
      const promise = action();
      await flushPromises();
      const frame = [...ws.sent].reverse().find((item) =>
        item.type === 'invoke-request' && (!method || item.cmd === method));
      if (!frame) throw new Error(`missing invoke frame ${method ?? ''}`);
      ws.receive({ type: 'invoke-result', _reqId: frame._reqId, _result: result });
      return promise;
    };

    expect(await resolveInvoke(() => conn.getTeammateTopology('workspace-a'), { roster: [], leaderId: null, edges: [] }, 'get_teammate_topology')).toEqual({ roster: [], leaderId: null, edges: [] });
    expect(await resolveInvoke(() => conn.sendAgentMessage({ workspaceId: 'workspace-a', paneId: 'agent' }, 'hello'), {
      messageId: 'm', deliveryId: 'd', targetKey: 'workspace-a/agent', status: 'delivered',
      deliveryAdapter: 'mcp_pull', deliveryReliability: 'durable', terminalAccepted: true, agentAcknowledged: false,
    }, 'send_agent_message')).toMatchObject({ messageId: 'm' });
    expect(await resolveInvoke(() => conn.listAgentHistory(200), [], 'read_agent_recent_replies')).toEqual([]);
    await resolveInvoke(() => conn.setTeammateGroups('workspace-a', []), null, 'set_teammate_groups');
    expect(await resolveInvoke(() => conn.resumeAgentSession('workspace-a', 'codex', 'session', '/repo'), { paneId: 'new-pane' }, 'resume_agent_session')).toBe('new-pane');
    expect(await resolveInvoke(() => conn.listHitlPending(), [], 'list_hitl_pending')).toEqual([]);
    expect(await resolveInvoke(() => conn.resolveHitlRemote('hitl', 'nonce', 'approve'), { outcome: 'consumed' }, 'resolve_hitl_remote')).toBe('consumed');
    expect(await resolveInvoke(() => conn.getOrchestrationHealth(), { suspendedAgents: 2, pendingHitl: 3 }, 'get_orchestration_health')).toEqual({ suspendedAgents: 2, pendingHitl: 3 });
    await resolveInvoke(() => conn.markPaneAgent('workspace-a', 'agent', true, 'agent-1'), null, 'register_teammate_agent');
    await resolveInvoke(() => conn.markPaneAgent('workspace-a', 'agent', false), null, 'release_teammate_agent');
    expect(await resolveInvoke(() => conn.listSavedWorkspaceFiles(), [{ name: 'x', path: '/x.ridge', mtime_secs: 4 }, { name: 'bad', path: '' }], 'list_saved_workspace_files')).toEqual([{ name: 'x', path: '/x.ridge', mtimeSecs: 4 }]);
    expect(await resolveInvoke(() => conn.openWorkspaceFromFile('/x.ridge'), 'workspace-b', 'open_workspace_from_file')).toBe('workspace-b');
    const project = conn.requestCurrentProject();
    await flushPromises();
    ws.receive({ type: 'current-project', path: '' });
    expect(await project).toBe('');

    conn.cycleTheme('dark');
    conn.setHostClipboard('copied');
    conn.listPanes();
    conn.listFiles('/repo');
    conn.listGitStatus();
    conn.resizePane('pane-a', 24, 80, 800, 400);
    conn.subscribePane(pane, { resume: true, sinceSeq: 3, active: true });
    conn.resyncPane(pane);
    expect(ws.sent).toEqual(expect.arrayContaining([
      expect.objectContaining({ type: 'cycle-theme', current: 'dark' }),
      expect.objectContaining({ type: 'set-host-clipboard', text: 'copied' }),
      expect.objectContaining({ type: 'list-files', path: '/repo' }),
      expect.objectContaining({ type: 'list-git-status' }),
      expect.objectContaining({ type: 'resize', paneId: 'pane-a' }),
    ]));
    conn.disconnect();
  });

  it('preserves structured Agent Hub errors for sendAgentMessage callers', async () => {
    const { conn, ws } = connect();
    const promise = conn.sendAgentMessage(
      { workspaceId: 'workspace-a', paneId: 'agent', agentId: 'agent-1', generation: 1, lease: 'lease-a' },
      'hello',
    );
    await flushPromises();
    const frame = invokeFrames(ws, 'send_agent_message')[0];
    if (!frame) throw new Error('missing send_agent_message invoke frame');
    ws.receive({
      type: 'invoke-result',
      _reqId: frame._reqId,
      _error: { code: 'BUSY', message: 'agent is busy' },
    });
    await expect(promise).rejects.toThrow('{"code":"BUSY","message":"agent is busy"}');
    conn.disconnect();
  });
});
