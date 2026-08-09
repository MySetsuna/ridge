import { describe, expect, it, vi, beforeEach } from 'vitest';
import { get } from 'svelte/store';

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
    markPaneAgent: vi.fn(async () => { calls.push('mark-agent'); }),
    listShells: vi.fn(async () => [{ id: 'bash', label: 'Bash', program: 'bash', args: [] }]),
    changePaneShell: vi.fn(async () => { calls.push('change-shell'); }),
  };
  return { link, calls };
}

beforeEach(() => {
  mockInvoke.mockReset();
  hosts.hostsStore.set([]);
  hosts.outboundPumpByHost.set({});
  hosts.outboundLifecycleByKey.set({});
  hosts.foreignHistoryByKey.set({});
  hosts.hostConnectProgress.set(null);
});

describe('hosts store public helper projections', () => {
  it('scopes attached panes and models pump, lifecycle, history, and reconnect state', async () => {
    expect(hosts.remotePaneKey('w', 'p')).toBe('w\0p');
    const index = hosts.collectAttachedRemotePanes([
      { workspaceId: 'w', remoteSessionId: 'p', attached: true },
      { remoteSessionId: 'legacy', attached: true },
      { workspaceId: 'w', remoteSessionId: 'off', attached: false },
    ]);
    expect(hosts.isRemotePaneAttached(index, 'w', 'p', 2)).toBe(true);
    expect(hosts.isRemotePaneAttached(index, 'other', 'legacy', 1)).toBe(true);
    expect(hosts.isRemotePaneAttached(index, 'other', 'legacy', 2)).toBe(false);

    hosts.notePumpBatch('h', 100);
    hosts.notePumpBatch('h', 200);
    expect(get(hosts.outboundPumpByHost).h?.bufferedBytes).toBe(300);
    expect(hosts.pumpBadgeForHost('h')).toBe('');
    hosts.noteLifecycleSubscribe('h', 's');
    hosts.noteLifecycleFanout('h', 12);
    expect(get(hosts.outboundLifecycleByKey)['h\0s']?.subscribed).toBe(true);
    hosts.noteLifecycleDetach('h', 's');
    expect(get(hosts.outboundLifecycleByKey)['h\0s']?.subscribed).toBe(false);
    expect(hosts.attachSeedPlanForSession('h', 's', false).seedBeforeLive).toBe(false);

    mockInvoke.mockResolvedValueOnce({ hostId: 'h', sessionId: 's', bytes: 64, cap: 128, dataB64: 'YQ==' });
    expect(await hosts.fetchForeignHistoryTail('h', 's')).toMatchObject({ bytes: 64, cap: 128 });
    expect(hosts.historyBadgeForSession('h', 's')).not.toBe('');
    expect(hosts.attachSeedPlanForSession('h', 's', true).seedBeforeLive).toBe(true);
    expect(hosts.noteOutboundReconnectAttempt('h')).toBe(1);
    expect(hosts.noteOutboundReconnectAttempt('h')).toBe(2);
    hosts.resetOutboundReconnectAttempt('h');
    expect(get(hosts.outboundReconnectAttempts).h).toBeUndefined();
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

  it('attaches and detaches a remote session without confusing view lifecycle with PTY termination', async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === 'get_foreign_history_tail') {
        return { hostId: 'remote-attach', sessionId: 'session-1', bytes: 0, cap: 128, dataB64: '' };
      }
      if (command === 'attach_host_session') return 'local-foreign-pane';
      if (command === 'list_native_sessions' || command === 'host_list_snapshot') return [];
      return undefined;
    });

    await expect(hosts.attachRemoteHostSession('remote-attach', 'session-1')).resolves.toBe('local-foreign-pane');
    expect(mockInvoke).toHaveBeenCalledWith('attach_host_session', {
      hostId: 'remote-attach', sessionId: 'session-1', workspaceId: '',
    });
    expect(get(hosts.outboundLifecycleByKey)['remote-attach\0session-1']?.subscribed).toBe(true);

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
    expect(get(hosts.outboundLifecycleByKey)['remote-attach\0session-1']?.subscribed).toBe(false);
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
    await hosts.markHostPaneAgent('remote-1', 'w1', 'p1', true);
    await hosts.changeHostPaneShell('remote-1', 'w1', 'p1', 'bash');
    expect(await hosts.hostShellChoices('remote-1')).toEqual([{ id: 'bash', label: 'Bash' }]);
    expect(calls).toEqual(expect.arrayContaining([
      'create-workspace', 'switch', 'rename', 'save', 'create-pane',
      'close-workspace', 'mark-agent', 'change-shell',
    ]));
    hosts.cancelHostTopologyRetry('remote-1');
    await hosts.retryHostTopology('remote-1');
    remove();
    expect(hosts.hasHostTopologyLink('remote-1')).toBe(false);
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

  it('updates outbound stats/backpressure and returns safe defaults on IPC errors', async () => {
    const stats = {
      hostId: 'h', state: 'connected', subscribed: ['s'], helloOk: 1, listOk: 1,
      subscribeOk: 1, writeOk: 2, resizeOk: 1, fanoutBytes: 9, reconnectAttempts: 0,
      resubscribeOk: 1, errors: 0, liveBufferCap: 128, liveBufferBytes: 32,
      liveDroppedBytes: 4,
    };
    mockInvoke.mockResolvedValueOnce(stats);
    expect(await hosts.fetchOutboundStats('h')).toEqual(stats);
    expect(get(hosts.outboundPumpByHost).h?.bufferedBytes).toBe(32);
    const bp = { hostId: 'h', cap: 128, buffered: 16, dropped: 5, highWater: 64, sessions: 1, sheddingSessions: 0, level: 'normal', totalDroppedGlobal: 5, injects: 0 };
    mockInvoke.mockResolvedValueOnce(bp);
    expect(await hosts.fetchLiveBackpressure('h')).toEqual(bp);
    expect(get(hosts.liveBackpressureByHost).h).toEqual(bp);
    mockInvoke.mockResolvedValueOnce(17);
    expect(await hosts.pumpHostOutput('h')).toBe(17);
    mockInvoke.mockRejectedValue(new Error('offline'));
    expect(await hosts.fetchOutboundStats('h')).toBeNull();
    expect(await hosts.fetchLiveBackpressure('h')).toBeNull();
    expect(await hosts.fetchForeignHistoryTail('h', 's')).toBeNull();
    expect(await hosts.pumpHostOutput('h')).toBe(0);
    expect(hosts.hostOperatorAlert('unknown', 0)).toBeNull();
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

  it('pumps only connected unlinked hosts and projects lifecycle alerts', async () => {
    hosts.hostsStore.set([
      { id: 'remote-live', kind: 'remote', label: 'Live', status: 'connected', sessions: [], workspaces: [] },
      { id: 'remote-offline', kind: 'remote', label: 'Offline', status: 'disconnected', sessions: [], workspaces: [] },
      { id: 'headless', kind: 'headless', label: 'Local', status: 'connected', sessions: [], workspaces: [] },
    ]);
    mockInvoke.mockResolvedValueOnce(64);
    expect(await hosts.pumpAllConnectedOutbound()).toBe(64);
    expect(mockInvoke).toHaveBeenCalledWith('pump_host_output', { hostId: 'remote-live' });
    expect(hosts.pumpBadgeForHost('remote-live')).toBe('');

    hosts.noteLifecycleSubscribe('remote-live', 'session-1');
    hosts.noteLifecycleFanout('remote-live', 128);
    expect(get(hosts.outboundLifecycleByKey)['remote-live\0session-1']?.fanoutBytes).toBe(128);
    hosts.noteOutboundReconnectAttempt('remote-live');
    expect(hosts.hostOperatorAlert('remote-live', 1)).toEqual(expect.any(String));
  });
});
