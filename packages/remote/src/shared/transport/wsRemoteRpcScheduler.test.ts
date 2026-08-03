import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { RemoteConnection, type PaneRef } from './wsRemote';

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
    expect(invokeFrames(ws, 'write_to_pty')).toHaveLength(1);

    await vi.advanceTimersByTimeAsync(8);
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
});
