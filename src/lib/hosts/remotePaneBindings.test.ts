import { afterEach, describe, expect, it, vi } from 'vitest';
import type { HostTopologyLink } from './hostForest';
import {
  bindRemotePane,
  deleteRemotePane,
  localPaneIdsForRemote,
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
});
