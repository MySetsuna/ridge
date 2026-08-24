import { afterEach, describe, expect, it, vi } from 'vitest';
import type { HostTopologyLink } from './hostForest';

const { feed } = vi.hoisted(() => ({ feed: vi.fn() }));
vi.mock('@ridge/remote/shared/terminal/manager', () => ({
  TerminalManager: { instance: vi.fn(() => ({ feed, enqueueFeed: feed })) },
}));

import {
  activateRemotePaneBinding,
  bindRemotePane,
  deleteRemotePane,
  localPaneIdsForRemote,
  remotePaneBinding,
  promoteRemotePaneBinding,
  terminalPathOrigin,
  unbindRemotePane,
} from './remotePaneBindings';

function link(): HostTopologyLink {
  return {
    state: () => 'connected',
    disconnect: vi.fn(),
    listWorkspaces: vi.fn(),
    listWorkspacePanes: vi.fn(),
    switchWorkspace: vi.fn(),
    createWorkspace: vi.fn(),
    createPane: vi.fn(),
    closePane: vi.fn(),
    closeWorkspace: vi.fn(),
    onRawBytes: vi.fn(() => () => {}),
    subscribePane: vi.fn(),
    sendStdin: vi.fn(),
    refreshPane: vi.fn(),
    getPaneOutput: vi.fn(() => []),
  };
}

const ids: string[] = [];
afterEach(() => {
  for (const id of ids.splice(0)) unbindRemotePane(id);
});

describe('remotePaneBindings', () => {
  it('projects the bound host filesystem probe without changing origin', async () => {
    const inspectPath = vi.fn(async () => ({ exists: true, isDirectory: false }));
    const remote = { ...link(), inspectPath };
    ids.push('local-origin');
    bindRemotePane({
      localPaneId: 'local-origin',
      hostId: 'rdg:device',
      workspaceId: 'remote-workspace',
      remotePaneId: 'remote-pane',
      link: remote,
    });
    const origin = terminalPathOrigin('local-origin');
    expect(origin).toMatchObject({ kind: 'rdg', hostId: 'rdg:device' });
    await expect(origin?.inspectPath?.('/srv/a.ts')).resolves.toEqual({
      exists: true,
      isDirectory: false,
    });
    expect(inspectPath).toHaveBeenCalledWith('/srv/a.ts');
  });

  it('returns no binding for unknown panes and ignores unknown lifecycle events', () => {
    expect(remotePaneBinding('missing')).toBeUndefined();
    activateRemotePaneBinding('missing');
    promoteRemotePaneBinding('missing');
    unbindRemotePane('missing');
  });

  it('buffers matching output, drops old bytes at the cap, then flushes on activation', () => {
    const remote = link();
    const onRawBytes = vi.mocked(remote.onRawBytes);
    let receive: Parameters<NonNullable<HostTopologyLink['onRawBytes']>>[0];
    onRawBytes.mockImplementationOnce((listener) => {
      receive = listener;
      return () => {};
    });
    vi.mocked(remote.getPaneOutput).mockReturnValueOnce(['initial']);
    bindRemotePane({
      localPaneId: 'local-buffered',
      hostId: 'host-buffered',
      workspaceId: 'workspace-buffered',
      remotePaneId: 'remote-buffered',
      link: remote,
    });
    ids.push('local-buffered');
    expect(remotePaneBinding('local-buffered')?.remotePaneId).toBe('remote-buffered');

    const first = new Uint8Array(600_000).fill(1);
    const second = new Uint8Array(600_000).fill(2);
    receive!({ paneId: 'other', workspaceId: 'workspace-buffered' }, first);
    receive!({ paneId: 'remote-buffered', workspaceId: 'workspace-buffered' }, first);
    receive!({ paneId: 'remote-buffered', workspaceId: 'workspace-buffered' }, second);

    activateRemotePaneBinding('local-buffered');
    expect(feed).toHaveBeenCalledTimes(1);
    expect(feed).toHaveBeenCalledWith('local-buffered', second);
    expect(remote.promotePane).not.toBeDefined();
  });

  it('forwards active bytes and promotes the remote pane', () => {
    const remote = link();
    const receive = vi.fn();
    vi.mocked(remote.onRawBytes).mockImplementationOnce((listener) => {
      receive.mockImplementation(listener);
      return () => {};
    });
    remote.promotePane = vi.fn();
    bindRemotePane({
      localPaneId: 'local-active',
      hostId: 'host-active',
      workspaceId: 'workspace-active',
      remotePaneId: 'remote-active',
      link: remote,
    });
    ids.push('local-active');
    activateRemotePaneBinding('local-active');
    promoteRemotePaneBinding('local-active');

    const bytes = Uint8Array.from([7, 8]);
    receive({ paneId: 'remote-active', workspaceId: 'workspace-active' }, bytes);
    expect(feed).toHaveBeenCalledWith('local-active', bytes);
    expect(remote.promotePane).toHaveBeenCalledWith({ workspaceId: 'workspace-active', paneId: 'remote-active' });
  });

  it('keeps host connected until its last local pane is removed', () => {
    const remote = link();
    for (const [localPaneId, remotePaneId] of [['local-a', 'remote-a'], ['local-b', 'remote-b']]) {
      ids.push(localPaneId);
      bindRemotePane({
        localPaneId,
        hostId: 'host-a',
        workspaceId: 'workspace-a',
        remotePaneId,
        link: remote,
      });
    }

    unbindRemotePane('local-a');
    expect(remote.disconnect).not.toHaveBeenCalled();
    expect(localPaneIdsForRemote('host-a', 'remote-b')).toEqual(['local-b']);

    unbindRemotePane('local-b');
    expect(remote.disconnect).toHaveBeenCalledTimes(1);
  });

  it('does not reduce attachments when remote deletion fails', async () => {
    const remote = link();
    vi.mocked(remote.closePane).mockResolvedValue(false);
    ids.push('local-failed');
    bindRemotePane({
      localPaneId: 'local-failed',
      hostId: 'host-failed',
      workspaceId: 'workspace-failed',
      remotePaneId: 'remote-failed',
      link: remote,
    });
    const closeLocal = vi.fn(async () => unbindRemotePane('local-failed'));

    await expect(
      deleteRemotePane('host-failed', 'remote-failed', remote, closeLocal),
    ).resolves.toBe(false);
    expect(closeLocal).not.toHaveBeenCalled();
    expect(localPaneIdsForRemote('host-failed', 'remote-failed')).toEqual(['local-failed']);
    expect(remote.disconnect).not.toHaveBeenCalled();
  });

  it('closes the remote pane and its local attachments with the modern signature', async () => {
    const remote = link();
    vi.mocked(remote.closePane).mockResolvedValue(true);
    const closeLocal = vi.fn(async (localPaneId: string) => unbindRemotePane(localPaneId));
    ids.push('local-modern');
    bindRemotePane({
      localPaneId: 'local-modern',
      hostId: 'host-modern',
      workspaceId: 'workspace-modern',
      remotePaneId: 'remote-modern',
      link: remote,
    });

    await expect(deleteRemotePane(
      'host-modern',
      'workspace-modern',
      'remote-modern',
      remote,
      closeLocal,
    )).resolves.toBe(true);
    expect(remote.closePane).toHaveBeenCalledWith({ workspaceId: 'workspace-modern', paneId: 'remote-modern' });
    expect(closeLocal).toHaveBeenCalledWith('local-modern');
    expect(localPaneIdsForRemote('host-modern', 'remote-modern')).toEqual([]);
  });

  it('rejects links without a closePane capability', async () => {
    const remote = { ...link(), closePane: undefined } as unknown as HostTopologyLink;

    await expect(deleteRemotePane(
      'host-no-close',
      'workspace-no-close',
      'remote-no-close',
      remote,
      vi.fn(async () => {}),
    )).resolves.toBe(false);
  });
});
