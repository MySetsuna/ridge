import { describe, expect, it, vi, beforeEach } from 'vitest';
import { get } from 'svelte/store';
import { activeSharedWorkspaceProjection } from '$lib/remote/cloud/sharedWorkspaceProjection';

const mockInvoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
  isTauri: () => false,
}));

const hosts = await import('./hosts');

function fakeLink() {
  let state = 'connected';
  const calls: string[] = [];
  const link = {
    state: () => state,
    disconnect: () => { state = 'disconnected'; calls.push('disconnect'); },
    listWorkspaces: vi.fn(async () => ({
      workspaces: [{ id: 'w1', name: ' Workspace ', active: true }],
    })),
    listWorkspacePanes: vi.fn(async () => [{ id: 'p1', title: ' Pane ', cwd: '/tmp', isAgent: true }]),
    switchWorkspace: vi.fn(async () => { calls.push('switch'); return true; }),
    createWorkspace: vi.fn(async () => { calls.push('create-workspace'); return 'w2'; }),
    renameWorkspace: vi.fn(async () => { calls.push('rename'); return true; }),
    saveWorkspace: vi.fn(async () => { calls.push('save'); return true; }),
    createPane: vi.fn(async () => { calls.push('create-pane'); return 'p2'; }),
    closePane: vi.fn(async () => { calls.push('close-pane'); return true; }),
    closeWorkspace: vi.fn(async () => { calls.push('close-workspace'); return true; }),
    onRawBytes: vi.fn(() => () => undefined),
    subscribePane: vi.fn(),
    sendStdin: vi.fn(),
    refreshPane: vi.fn(),
    getPaneOutput: vi.fn(() => []),
    listShells: vi.fn(async () => [{ id: 'bash', label: 'Bash', program: 'bash', args: [] }]),
    changePaneShell: vi.fn(async () => { calls.push('change-shell'); }),
  };
  return { link, calls };
}

beforeEach(() => {
  mockInvoke.mockReset();
  hosts.hostsStore.set([]);
  hosts.hostConnectProgress.set(null);
});

describe('hosts store public helper projections', () => {
  it('keeps Remote headless sessions visible without attempting native Host topology', async () => {
    vi.stubEnv('RIDGE_WEB_REMOTE', '1');
    try {
      mockInvoke.mockImplementation(async (command: string) => {
        if (command === 'list_native_sessions') return [];
        throw new Error(`unexpected Remote command: ${command}`);
      });
      await hosts.refreshHosts();
      expect(get(hosts.hostsStore).map(host => host.kind)).toEqual(['headless']);
      expect(get(hosts.hostsError)).toBe('');
      expect(mockInvoke).not.toHaveBeenCalledWith('host_list_snapshot');
    } finally { vi.unstubAllEnvs(); }
  });
  it('scopes attached panes by workspace', async () => {
    expect(hosts.remotePaneKey('w', 'p')).toBe('w\0p');
    const index = hosts.collectAttachedRemotePanes([
      { workspaceId: 'w', remoteSessionId: 'p', attached: true },
      { remoteSessionId: 'legacy', attached: true },
      { workspaceId: 'w', remoteSessionId: 'off', attached: false },
    ]);
    expect(hosts.isRemotePaneAttached(index, 'w', 'p', 2)).toBe(true);
    expect(hosts.isRemotePaneAttached(index, 'other', 'legacy', 1)).toBe(true);
    expect(hosts.isRemotePaneAttached(index, 'other', 'legacy', 2)).toBe(false);

  });

  it('refreshes native snapshots and routes headless session commands', async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === 'list_native_sessions') {
        return [{ socket: 'tmux', name: 'shell', windows: 1, panes: 1, width: 80, height: 24, attached: false }];
      }
      if (command === 'host_list_snapshot') return [];
      if (command === 'new_headless_session') return 'created';
      if (command === 'summon_native_session') return 7;
      return undefined;
    });

    await hosts.refreshHosts();
    expect(get(hosts.hostsStore)).toEqual([expect.objectContaining({
      id: 'headless', kind: 'headless', status: 'connected',
    })]);
    expect(await hosts.newHeadlessSession(' shell ', ' /tmp ')).toBe('created');
    await hosts.attachSession('tmux', 'shell');
    await hosts.terminateSession('tmux', 'shell');
    expect(mockInvoke).toHaveBeenCalledWith('terminate_native_session', { socket: 'tmux', target: 'shell' });
  });

  it('docks the actual new native view when summon returns a count, not a global ID', async () => {
    const panes = await import('./paneTree');
    const dock = vi.spyOn(panes, 'dockPane').mockResolvedValue();
    const sync = vi.spyOn(panes, 'syncPaneLayoutFromBackend').mockResolvedValue();
    const target = { type: 'leaf', id: 'target' };
    const adopted = { type: 'leaf', id: 'actual-view', origin: {
      kind: 'headless', host_id: 'headless', host_label: 'headless', session_id: 'headless:47',
    } };
    let reads = 0;
    mockInvoke.mockImplementation(async command => {
      if (command === 'get_pane_layout_for') return ++reads === 1 ? target : {
        type: 'split', id: 'split', direction: 'horizontal', children: [target, adopted], ratios: [0.5, 0.5],
      };
      if (command === 'summon_native_session') return 1;
      return [];
    });
    try {
      expect(await hosts.attachHostSession({ kind: 'headless', socket: 'headless', target: 'shell',
        workspaceId: 'workspace', targetPaneId: 'target', region: 'right',
      })).toBe('actual-view');
      expect(dock).toHaveBeenCalledWith('actual-view', 'target', 'right');
      expect(mockInvoke).toHaveBeenCalledWith('get_pane_layout_for', { workspaceId: 'workspace' });
    } finally { dock.mockRestore(); sync.mockRestore(); }
  });

  it('refuses to dock an ambiguous native view after concurrent layout changes', async () => {
    const panes = await import('./paneTree');
    const dock = vi.spyOn(panes, 'dockPane').mockResolvedValue();
    let reads = 0;
    mockInvoke.mockImplementation(async command => {
      if (command === 'get_pane_layout_for') return ++reads === 1 ? { type: 'leaf', id: 'target' } : {
        type: 'split', id: 'split', direction: 'horizontal', ratios: [0.5, 0.5], children: ['first', 'concurrent'].map(id => ({
          type: 'leaf', id, origin: { kind: 'headless', host_id: 'headless', host_label: 'headless', session_id: `headless:${id}` },
        })),
      };
      if (command === 'summon_native_session') return 1;
      return [];
    });
    try {
      await expect(hosts.attachHostSession({ kind: 'headless', socket: 'headless', target: 'shell',
        workspaceId: 'workspace', targetPaneId: 'target', region: 'right',
      })).rejects.toThrow('Native layout changed');
      expect(dock).not.toHaveBeenCalled();
    } finally { dock.mockRestore(); }
  });

  it('attaches and detaches a remote session without confusing view lifecycle with PTY termination', async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === 'attach_host_session') return 'local-foreign-pane';
      if (command === 'list_native_sessions' || command === 'host_list_snapshot') return [];
      return undefined;
    });

    await expect(hosts.attachRemoteHostSession('remote-attach', 'session-1')).resolves.toBe('local-foreign-pane');
    expect(mockInvoke).toHaveBeenCalledWith('attach_host_session', {
      hostId: 'remote-attach', sessionId: 'session-1', workspaceId: '',
    });

    hosts.hostsStore.set([{
      id: 'remote-attach', kind: 'remote', label: 'Remote', status: 'connected', sessions: [{
        socket: 'remote-attach', name: 'Agent', remoteSessionId: 'session-1',
        windows: 0, panes: 1, width: 80, height: 24, attached: true,
      }], workspaces: [],
    }]);
    await hosts.detachRemoteHostSession('local-foreign-pane');
    expect(mockInvoke).toHaveBeenCalledWith('detach_host_session', {
      paneId: 'local-foreign-pane', workspaceId: '',
    });
  });

  it('publishes linked topology and performs remote workspace mutations', async () => {
    const { link, calls } = fakeLink();
    const remove = hosts.registerHostTopologyLink({
      hostId: 'remote-1', kind: 'remote', label: 'Remote', link,
    });
    expect(hosts.hasHostTopologyLink('remote-1')).toBe(true);
    expect(hosts.hostShareDeviceName('remote-1')).toBeUndefined();
    await hosts.refreshHostTopology('remote-1');
    const host = get(hosts.hostsStore).find((item) => item.id === 'remote-1');
    expect(host).toMatchObject({ status: 'connected', workspaces: [{ id: 'w1', name: 'Workspace' }] });
    expect(host?.sessions[0]).toMatchObject({ remoteSessionId: 'p1', attached: false, cwd: '/tmp' });

    await hosts.createHostWorkspace('remote-1', 'new');
    await hosts.openHostWorkspace('remote-1', 'w1');
    await hosts.renameHostWorkspace('remote-1', 'w1', 'renamed');
    await hosts.saveHostWorkspace('remote-1', 'w1', 'saved');
    await hosts.createHostPane('remote-1', 'w1');
    await hosts.closeHostWorkspace('remote-1', 'w1');
    await hosts.changeHostPaneShell('remote-1', 'w1', 'p1', 'bash');
    expect(await hosts.hostShellChoices('remote-1')).toEqual([{ id: 'bash', label: 'Bash' }]);
    expect(calls).toEqual(expect.arrayContaining([
      'create-workspace', 'switch', 'rename', 'save', 'create-pane',
      'close-workspace', 'change-shell',
    ]));
    hosts.cancelHostTopologyRetry('remote-1');
    await hosts.retryHostTopology('remote-1');
    remove();
    expect(hosts.hasHostTopologyLink('remote-1')).toBe(false);
  });

  it('single-flights repeated topology retries and permits a retry after cancellation', async () => {
    const { link } = fakeLink();
    const remove = hosts.registerHostTopologyLink({
      hostId: 'retry-single-flight', kind: 'remote', label: 'Retry', link,
    });

    const first = hosts.retryHostTopology('retry-single-flight');
    const second = hosts.retryHostTopology('retry-single-flight');
    await Promise.all([first, second]);
    expect(link.listWorkspaces).toHaveBeenCalledTimes(1);

    hosts.cancelHostTopologyRetry('retry-single-flight');
    await hosts.retryHostTopology('retry-single-flight');
    expect(link.listWorkspaces).toHaveBeenCalledTimes(2);
    remove();
  });

  it('fails closed for invalid connection/share input and tracks link disconnect', async () => {
    await expect(hosts.connectHost('remote', '', '127.0.0.1', '')).rejects.toThrow('LAN 主机地址或 TOTP 无效');
    expect(get(hosts.hostConnectProgress)).toMatchObject({ phase: 'error' });
    await expect(hosts.acceptSharedWorkspace('grant')).rejects.toThrow('请先登录 Ridge Cloud');
    await expect(hosts.revokeSharedWorkspace('grant')).rejects.toThrow('请先登录 Ridge Cloud');
    await expect(hosts.openSharedWorkspace({ name: 'x', socket: 's', windows: 0, panes: 0, width: 0, height: 0, attached: false })).rejects.toThrow('共享工作区信息不完整');

    const { link } = fakeLink();
    const remove = hosts.registerHostTopologyLink({ hostId: 'remote-2', kind: 'rdg', label: 'Rdg', link });
    await hosts.disconnectHost('remote-2');
    expect(hosts.hasHostTopologyLink('remote-2')).toBe(true);
    remove();
  });

  it('fences replaced topology links and honors manual disconnect', async () => {
    const first = fakeLink();
    const removeFirst = hosts.registerHostTopologyLink({
      hostId: 'replace-me', kind: 'remote', label: 'First', link: first.link,
    });
    const second = fakeLink();
    const removeSecond = hosts.registerHostTopologyLink({
      hostId: 'replace-me', kind: 'rdg', label: 'Second', link: second.link,
      manualDisconnected: true,
    });
    expect(first.calls).toContain('disconnect');
    removeFirst();
    expect(hosts.hasHostTopologyLink('replace-me')).toBe(true);
    expect(await hosts.refreshHostTopology('replace-me')).toBeNull();
    removeSecond();
    expect(hosts.hasHostTopologyLink('replace-me')).toBe(false);
  });

  it('projects the active shared workspace into its grouped host', () => {
    hosts.hostsStore.set([{
      id: 'shared:owner:device', kind: 'shared', label: 'Owner · device', status: 'connected',
      sessions: [{
        socket: 'shared:owner:device', name: 'old', workspaceId: 'workspace-1',
        shareGrantId: 'grant-1', shareStatus: 'active', ownerUsername: 'owner',
        deviceName: 'device', windows: 1, panes: 0, width: 0, height: 0, attached: false,
      }],
      workspaces: [{
        id: 'workspace-1', name: 'Old', active: true, shareGrantId: 'grant-1',
        shareStatus: 'active', role: 'operator', sessions: [],
      }],
    }]);

    activeSharedWorkspaceProjection.set({
      grantId: 'grant-1', workspaceId: 'workspace-1', name: 'Projected',
      ownerUsername: 'owner', deviceName: 'device',
      panes: [{ id: 'pane-1', title: '', cwd: '/repo', isAgent: true }],
      link: {} as never,
      dataProvider: {} as never,
    });

    const host = get(hosts.hostsStore)[0];
    expect(host.sessions).toEqual([expect.objectContaining({
      remoteSessionId: 'pane-1', name: 'pane-1', cwd: '/repo', isAgent: true, attached: true,
    })]);
    expect(host.workspaces[0]).toMatchObject({ name: 'Projected', sessions: host.sessions });
    activeSharedWorkspaceProjection.set(null);
  });

  it('fails closed for unsupported or rejected remote mutations', async () => {
    await expect(hosts.createHostWorkspace('missing', 'x')).rejects.toThrow();
    const { link } = fakeLink();
    const remove = hosts.registerHostTopologyLink({ hostId: 'mutation-errors', kind: 'remote', label: 'Errors', link });

    link.switchWorkspace.mockResolvedValue(false);
    await expect(hosts.openHostWorkspace('mutation-errors', 'w1')).rejects.toThrow();
    await expect(hosts.createHostPane('mutation-errors', 'w1')).rejects.toThrow();
    link.switchWorkspace.mockResolvedValue(true);
    link.createPane.mockResolvedValue('');
    await expect(hosts.createHostPane('mutation-errors', 'w1')).rejects.toThrow();

    link.renameWorkspace.mockResolvedValue(false);
    await expect(hosts.renameHostWorkspace('mutation-errors', 'w1', 'x')).rejects.toThrow();
    link.saveWorkspace.mockResolvedValue(false);
    await expect(hosts.saveHostWorkspace('mutation-errors', 'w1', 'x')).rejects.toThrow();
    link.closeWorkspace.mockResolvedValue(false);
    await expect(hosts.closeHostWorkspace('mutation-errors', 'w1')).rejects.toThrow();

    link.listShells.mockResolvedValue([]);
    await expect(hosts.changeHostPaneShell('mutation-errors', 'w1', 'p1', 'missing')).rejects.toThrow();
    link.listShells = undefined as never;
    link.changePaneShell = undefined as never;
    await expect(hosts.changeHostPaneShell('mutation-errors', 'w1', 'p1', 'bash')).rejects.toThrow();
    await expect(hosts.hostShellChoices('mutation-errors')).resolves.toEqual([]);
    remove();
  });
});
