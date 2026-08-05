import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CloudWebrtcAdapter, PaneRef, RpcClient, RpcRequestOptions } from '@ridge/remote';
import type { PaneNode } from '$lib/types';
import { CloudHostTopologyLink } from './cloudHostTopologyLink';

class FakeRpc {
  requests: Array<{ method: string; params: unknown; options?: RpcRequestOptions }> = [];
  cancelledScopes: string[] = [];
  rejectClose = false;
  rejectWorkspaceClose = false;
  rejectWorkspaceCreate = false;
  paneLayout: PaneNode | null = null;
  notifications: Array<{ method: string; params: unknown }> = [];
  reconnectHooks = new Set<() => void>();

  request(method: string, params?: unknown, options?: RpcRequestOptions): Promise<unknown> {
    this.requests.push({ method, params, options });
    if (method === 'close_pane') {
      return this.rejectClose ? Promise.reject(new Error('close failed')) : Promise.resolve(null);
    }
    if (method === 'close_workspace') {
      return this.rejectWorkspaceClose
        ? Promise.reject(new Error('workspace close failed'))
        : Promise.resolve(null);
    }
    if (method === 'create_workspace') {
      return this.rejectWorkspaceCreate
        ? Promise.reject(new Error('workspace create failed'))
        : Promise.resolve('workspace-new');
    }
    if (method === 'get_pane_layout_for') return Promise.resolve(this.paneLayout);
    return new Promise(() => {});
  }

  notify(method: string, params?: unknown): void {
    this.notifications.push({ method, params });
  }
  dispose(): void {}
  onReconnected(hook: () => void): () => void {
    this.reconnectHooks.add(hook);
    return () => this.reconnectHooks.delete(hook);
  }
  cancelScope(scope: string): number {
    this.cancelledScopes.push(scope);
    return 2;
  }
}

function adapter(): CloudWebrtcAdapter {
  return {
    state: () => 'connected',
    close: vi.fn(),
    dispose: vi.fn(),
  } as unknown as CloudWebrtcAdapter;
}

function linkWith(rpc: FakeRpc): CloudHostTopologyLink {
  return new CloudHostTopologyLink(adapter(), rpc as unknown as RpcClient);
}

describe('CloudHostTopologyLink pane lifecycle', () => {
  const paneA: PaneRef = { workspaceId: 'w1', paneId: 'p1' };
  const paneB: PaneRef = { workspaceId: 'w1', paneId: 'p2' };

  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('preserves Agent state, id, and CWD in the cloud pane projection', async () => {
    const rpc = new FakeRpc();
    rpc.paneLayout = {
      type: 'split',
      id: 'root',
      direction: 'horizontal',
      ratios: [50, 50],
      children: [
        { type: 'leaf', id: 'agent-pane', title: 'Agent', cwd: 'C:\\work\\agent', agent_state: 'busy', agent_id: 'agent-1' },
        { type: 'leaf', id: 'idle-pane', cwd: '/tmp', agent_state: 'idle' },
      ],
    };
    const link = linkWith(rpc);

    await expect(link.listWorkspacePanes('w1')).resolves.toEqual([
      { id: 'agent-pane', title: 'Agent', cwd: 'C:\\work\\agent', isAgent: true, agentState: 'busy', agentId: 'agent-1' },
      { id: 'idle-pane', title: undefined, cwd: '/tmp', isAgent: true, agentState: 'idle' },
    ]);
  });

  it('surfaces workspace create failures to the Host operation layer', async () => {
    const rpc = new FakeRpc();
    rpc.rejectWorkspaceCreate = true;
    const link = linkWith(rpc);

    await expect(link.createWorkspace('new')).rejects.toThrow('workspace create failed');
  });

  it('cancels one pane scope before close and blocks stale input/resize only for that pane', async () => {
    const rpc = new FakeRpc();
    const link = linkWith(rpc);
    link.subscribePane(paneA);
    link.subscribePane(paneB);
    link.sendStdin(paneA, 'a');
    link.refreshPane(paneA, 24, 80, 0, 0);
    await vi.advanceTimersByTimeAsync(40);

    const closing = link.closePane(paneA);
    link.sendStdin(paneA, 'stale');
    link.refreshPane(paneA, 30, 100, 0, 0);
    link.sendStdin(paneB, 'live');

    await expect(closing).resolves.toBe(true);
    expect(rpc.cancelledScopes).toEqual(['w1:p1']);
    expect(rpc.requests.filter(({ method }) => method === 'write_to_pty')).toHaveLength(2);
    expect(rpc.requests.filter(({ method }) => method === 'resize_pane')).toHaveLength(1);
    expect(rpc.requests.at(-1)?.params).toMatchObject({ paneId: 'p2', data: 'live' });
  });

  it('promotes the focused foreign pane without creating a second subscription', () => {
    const rpc = new FakeRpc();
    const link = linkWith(rpc);
    link.subscribePane(paneA);
    link.subscribePane(paneB);
    link.promotePane(paneA);
    link.promotePane(paneA);
    for (const reconnect of rpc.reconnectHooks) reconnect();
    link.promotePane(paneA);

    expect(rpc.notifications).toEqual([
      {
        method: 'subscribe-pane',
        params: { workspaceId: 'w1', paneId: 'p1', active: true },
      },
      {
        method: 'subscribe-pane',
        params: { workspaceId: 'w1', paneId: 'p2', active: true },
      },
      {
        method: 'subscribe-pane',
        params: { workspaceId: 'w1', paneId: 'p1', active: true },
      },
      {
        method: 'subscribe-pane',
        params: { workspaceId: 'w1', paneId: 'p1', active: true },
      },
      {
        method: 'subscribe-pane',
        params: { workspaceId: 'w1', paneId: 'p2', active: false },
      },
    ]);
  });

  it('replays only attached panes after reconnect and preserves focused QoS', async () => {
    const rpc = new FakeRpc();
    rpc.paneLayout = { type: 'leaf', id: 'discovered' };
    const link = linkWith(rpc);
    // Discovering a pane populates live state in production; attaching only A
    // must keep the replay set precise.
    await link.listWorkspacePanes('w1');
    link.subscribePane(paneA);
    for (const reconnect of rpc.reconnectHooks) reconnect();

    expect(rpc.notifications).toEqual([
      { method: 'subscribe-pane', params: { workspaceId: 'w1', paneId: 'p1', active: true } },
      { method: 'subscribe-pane', params: { workspaceId: 'w1', paneId: 'p1', active: true } },
    ]);
  });

  it('reactivates the pane when the host rejects close', async () => {
    const rpc = new FakeRpc();
    rpc.rejectClose = true;
    const link = linkWith(rpc);
    link.subscribePane(paneA);

    await expect(link.closePane(paneA)).resolves.toBe(false);
    link.sendStdin(paneA, 'retry');

    expect(rpc.requests.at(-1)).toMatchObject({
      method: 'write_to_pty',
      params: { workspaceId: 'w1', paneId: 'p1', data: 'retry' },
      options: { scope: 'w1:p1' },
    });
  });

  it('retires every pane in a closed workspace without touching another workspace', async () => {
    const rpc = new FakeRpc();
    const link = linkWith(rpc);
    const other: PaneRef = { workspaceId: 'w2', paneId: 'p3' };
    link.subscribePane(paneA);
    link.subscribePane(paneB);
    link.subscribePane(other);

    await expect(link.closeWorkspace('w1')).resolves.toBe(true);
    link.sendStdin(paneA, 'stale-a');
    link.sendStdin(paneB, 'stale-b');
    link.sendStdin(other, 'live');

    expect(rpc.cancelledScopes).toEqual(['w1:p1', 'w1:p2']);
    expect(rpc.requests.at(-1)?.params).toMatchObject({
      workspaceId: 'w2',
      paneId: 'p3',
      data: 'live',
    });
  });
});
