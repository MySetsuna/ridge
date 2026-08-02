import { describe, expect, it, vi } from 'vitest';
import type { PaneInfo, RemoteLink, WsMessage } from '@ridge/remote';
import {
  confirmedWorkspaceTarget,
  dedupeRemoteItems,
  mergeRemoteItems,
  requestPaneSnapshot,
  requestWorkspaceSnapshot,
} from './remoteQueries';

describe('remote query snapshots', () => {
  it('commits a composite target only after the host switch succeeds', async () => {
    await expect(confirmedWorkspaceTarget(async () => false, 'ws-b', 'pane-b'))
      .resolves.toBeNull();
    await expect(confirmedWorkspaceTarget(async () => true, 'ws-b', 'pane-b'))
      .resolves.toEqual({ workspaceId: 'ws-b', paneId: 'pane-b' });
  });

  it('dedupes canonical ids and lets the newest push win', () => {
    expect(dedupeRemoteItems([{ id: 'a', title: 'old' }, { id: 'a', title: 'new' }]))
      .toEqual([{ id: 'a', title: 'new' }]);
    expect(mergeRemoteItems([{ id: 'a', title: 'old' }], [
      { id: 'a', title: 'new' },
      { id: 'b', title: 'b' },
    ])).toEqual([{ id: 'a', title: 'new' }, { id: 'b', title: 'b' }]);
  });

  it('resolves only the requested workspace snapshot and detaches its listener', async () => {
    let listener!: (message: WsMessage) => void;
    const stop = vi.fn();
    const link = {
      onMessage(fn: (message: WsMessage) => void) {
        listener = fn;
        return stop;
      },
      listPanes: vi.fn(),
    } as unknown as RemoteLink;
    const pending = requestPaneSnapshot(link, 'ws-2');
    listener({ type: 'panes', workspaceId: 'ws-1', panes: [] });
    listener({ type: 'panes', panes: [{ id: 'unscoped', title: 'wrong' }] as PaneInfo[] });
    const expected = [{ id: 'p-2', title: 'two' }] as PaneInfo[];
    listener({ type: 'panes', workspaceId: 'ws-2', panes: expected });
    await expect(pending).resolves.toEqual(expected);
    expect(link.listPanes).toHaveBeenCalledOnce();
    expect(stop).toHaveBeenCalledOnce();
  });

  it('rejects and detaches when pane snapshot is aborted', async () => {
    const stop = vi.fn();
    const link = {
      onMessage: vi.fn(() => stop),
      listPanes: vi.fn(),
    } as unknown as RemoteLink;
    const controller = new AbortController();
    const pending = requestPaneSnapshot(link, 'ws-abort', controller.signal, 100);
    controller.abort();
    await expect(pending).rejects.toMatchObject({ name: 'AbortError' });
    expect(stop).toHaveBeenCalledOnce();
  });

  it('bounds pane and workspace snapshots when the transport never replies', async () => {
    const paneLink = {
      onMessage: vi.fn(() => vi.fn()),
      listPanes: vi.fn(),
    } as unknown as RemoteLink;
    await expect(requestPaneSnapshot(paneLink, 'ws-timeout', undefined, 1))
      .rejects.toThrow('list panes timed out');

    const workspaceLink = {
      listWorkspaces: vi.fn(() => new Promise<never>(() => undefined)),
    } as unknown as RemoteLink;
    await expect(requestWorkspaceSnapshot(workspaceLink, undefined, 1))
      .rejects.toThrow('list workspaces timed out');
  });
});
