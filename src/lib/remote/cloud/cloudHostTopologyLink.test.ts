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
  rejectSwitch = false;
  rejectRename = false;
  rejectSave = false;
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
    if (method === 'list_workspaces') return Promise.resolve([{ id: 'w1', name: 'One' }, { id: 'w2' }]);
    if (method === 'get_active_workspace_id') return Promise.resolve('w1');
    if (method === 'detect_available_shells') return Promise.resolve([{ id: 'bash', label: 'Bash', program: 'bash' }]);
    if (method === 'rename_workspace') {
      return this.rejectRename ? Promise.reject(new Error('rename failed')) : Promise.resolve(null);
    }
    if (method === 'save_workspace_to_file') {
      return this.rejectSave ? Promise.reject(new Error('save failed')) : Promise.resolve(null);
    }
    if (method === 'switch_workspace') {
      return this.rejectSwitch ? Promise.reject(new Error('switch failed')) : Promise.resolve(null);
    }
    if (method === 'register_teammate_agent' || method === 'release_teammate_agent') return Promise.resolve(null);
    if (method === 'split_pane') return Promise.resolve({ pane_id: 'pane-new' });
    if (method === 'change_pane_shell' || method === 'activate_pane_pty') return Promise.resolve(null);
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

interface FakeAdapter {
  state: () => 'connected';
  close: () => void;
  dispose: () => void;
  onPaneBytes: (fn: (paneId: string, bytes: Uint8Array) => void) => () => boolean;
  emitPaneBytes(paneId: string, bytes: Uint8Array): void;
}

function adapter(): FakeAdapter {
  const paneListeners = new Set<(paneId: string, bytes: Uint8Array) => void>();
  return {
    state: () => 'connected',
    close: vi.fn(),
    dispose: vi.fn(),
    onPaneBytes: (fn: (paneId: string, bytes: Uint8Array) => void) => {
      paneListeners.add(fn);
      return () => paneListeners.delete(fn);
    },
    emitPaneBytes: (paneId: string, bytes: Uint8Array) => {
      for (const fn of paneListeners) fn(paneId, bytes);
    },
  };
}

function linkWith(rpc: FakeRpc, transport: FakeAdapter = adapter()): CloudHostTopologyLink {
  return new CloudHostTopologyLink(transport as unknown as CloudWebrtcAdapter, rpc as unknown as RpcClient);
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

  it('creates a pane from the explicitly switched workspace layout', async () => {
    const rpc = new FakeRpc();
    rpc.paneLayout = { type: 'leaf', id: 'p1' };
    const link = linkWith(rpc);

    await expect(link.switchWorkspace('w2')).resolves.toBe(true);
    await expect(link.createPane()).resolves.toBe('pane-new');

    expect(rpc.requests).toContainEqual({
      method: 'get_pane_layout_for',
      params: { workspaceId: 'w2' },
      options: undefined,
    });
    expect(rpc.requests).toContainEqual({
      method: 'split_pane',
      params: { workspaceId: 'w2', paneId: 'p1', direction: 'horizontal' },
      options: undefined,
    });
  });

  it('passes the selected workspace when rebuilding a remote shell', async () => {
    const rpc = new FakeRpc();
    const link = linkWith(rpc);
    link.subscribePane(paneA);

    await link.changePaneShell('w1', 'p1', {
      id: 'git-bash',
      label: 'Git Bash',
      program: 'C:\\Program Files\\Git\\bin\\bash.exe',
      args: [],
    });

    expect(rpc.requests).toEqual([
      expect.objectContaining({
        method: 'change_pane_shell',
        params: {
          workspaceId: 'w1',
          paneId: 'p1',
          shell: 'C:\\Program Files\\Git\\bin\\bash.exe',
          args: [],
        },
        options: { scope: 'w1:p1' },
      }),
      expect.objectContaining({
        method: 'activate_pane_pty',
        params: { workspaceId: 'w1', paneId: 'p1' },
        options: { scope: 'w1:p1' },
      }),
    ]);
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
    for (const reconnect of rpc.reconnectHooks) reconnect();
    expect(rpc.notifications).toEqual([
      { method: 'subscribe-pane', params: { workspaceId: 'w1', paneId: 'p1', active: true } },
      { method: 'subscribe-pane', params: { workspaceId: 'w1', paneId: 'p1', active: true } },
    ]);
  });

  it('restores every attached subscription when workspace close is rejected', async () => {
    const rpc = new FakeRpc();
    rpc.rejectWorkspaceClose = true;
    const link = linkWith(rpc);
    link.subscribePane(paneA);
    link.subscribePane(paneB);

    await expect(link.closeWorkspace('w1')).resolves.toBe(false);
    for (const reconnect of rpc.reconnectHooks) reconnect();

    expect(rpc.notifications).toEqual([
      { method: 'subscribe-pane', params: { workspaceId: 'w1', paneId: 'p1', active: true } },
      { method: 'subscribe-pane', params: { workspaceId: 'w1', paneId: 'p2', active: true } },
      { method: 'subscribe-pane', params: { workspaceId: 'w1', paneId: 'p1', active: false } },
      { method: 'subscribe-pane', params: { workspaceId: 'w1', paneId: 'p2', active: true } },
    ]);
  });

  it('keeps same-named panes isolated during discovery cleanup', async () => {
    const rpc = new FakeRpc();
    const link = linkWith(rpc);
    const sameInOtherWorkspace: PaneRef = { workspaceId: 'w2', paneId: 'p1' };
    link.subscribePane(paneA);
    link.subscribePane(sameInOtherWorkspace);

    rpc.paneLayout = null;
    await expect(link.listWorkspacePanes('w1')).resolves.toEqual([]);

    expect(link.sendStdin(paneA, 'stale')).toBe(false);
    expect(link.sendStdin(sameInOtherWorkspace, 'live')).toBe(true);
  });

  it('routes legacy cloud raw bytes only when paneId identifies one workspace', async () => {
    const rpc = new FakeRpc();
    const transport = adapter();
    const link = linkWith(rpc, transport);
    const received: PaneRef[] = [];
    link.onRawBytes((pane) => received.push(pane));

    link.subscribePane(paneA);
    transport.emitPaneBytes('p1', new Uint8Array([1]));
    expect(received).toEqual([paneA]);

    const sameInOtherWorkspace: PaneRef = { workspaceId: 'w2', paneId: 'p1' };
    link.subscribePane(sameInOtherWorkspace);
    transport.emitPaneBytes('p1', new Uint8Array([2]));
    expect(received).toEqual([paneA]);

    await expect(link.closePane(paneA)).resolves.toBe(true);
    transport.emitPaneBytes('p1', new Uint8Array([3]));
    expect(received).toEqual([paneA, sameInOtherWorkspace]);
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

  it('lists workspaces/shells, forwards workspace mutations, and disconnects transport', async () => {
    const rpc = new FakeRpc();
    const transport = adapter();
    const link = linkWith(rpc, transport);

    expect(link.state()).toBe('connected');
    await expect(link.listWorkspaces()).resolves.toEqual({
      workspaces: [
        { id: 'w1', name: 'One', active: true },
        { id: 'w2', name: undefined, active: false },
      ],
    });
    await expect(link.listShells()).resolves.toEqual([{ id: 'bash', label: 'Bash', program: 'bash' }]);
    await expect(link.switchWorkspace('w2')).resolves.toBe(true);
    await expect(link.renameWorkspace('w2', 'Two')).resolves.toBe(true);
    await expect(link.saveWorkspace('w2', 'Two')).resolves.toBe(true);
    expect(link.getPaneOutput(paneA)).toEqual([]);
    expect(link.rpcSchedulingDiagnostics).toEqual(expect.any(Object));
    link.disconnect();
    expect(transport.close).toHaveBeenCalledOnce();
    expect(transport.dispose).toHaveBeenCalledOnce();
  });

  it('fails closed for workspace mutations and empty pane creation', async () => {
    const rpc = new FakeRpc();
    rpc.rejectSwitch = true;
    rpc.rejectRename = true;
    rpc.rejectSave = true;
    const link = linkWith(rpc);

    await expect(link.switchWorkspace('w2')).resolves.toBe(false);
    await expect(link.renameWorkspace('w2', 'Two')).resolves.toBe(false);
    await expect(link.saveWorkspace('w2', 'Two')).resolves.toBe(false);
    await expect(link.createPane()).resolves.toBeNull();
    await expect(link.markPaneAgent('w2', 'missing', true)).rejects.toThrow('Pane not active');
    await expect(link.changePaneShell('w2', 'missing', { id: 'x', label: 'x', program: 'x' })).rejects.toThrow('Pane not active');
  });

  it('routes agent registration and shell activation through the pane scope', async () => {
    const rpc = new FakeRpc();
    const link = linkWith(rpc);
    link.subscribePane(paneA);

    await link.markPaneAgent('w1', 'p1', true, 'agent-1');
    await link.markPaneAgent('w1', 'p1', false);
    await link.changePaneShell('w1', 'p1', { id: 'pwsh', label: 'PowerShell', program: 'pwsh' });

    expect(rpc.requests).toEqual(expect.arrayContaining([
      expect.objectContaining({
        method: 'register_teammate_agent',
        params: { workspaceId: 'w1', paneId: 'p1', agentId: 'agent-1' },
        options: { scope: 'w1:p1' },
      }),
      expect.objectContaining({
        method: 'release_teammate_agent',
        params: { workspaceId: 'w1', paneId: 'p1' },
        options: { scope: 'w1:p1' },
      }),
      expect.objectContaining({ method: 'activate_pane_pty', options: { scope: 'w1:p1' } }),
    ]));
  });
});
