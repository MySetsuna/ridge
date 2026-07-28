import { describe, expect, it, vi } from 'vitest';
import type { PaneInfo, RemoteLink, WsMessage } from '@ridge/remote';
import {
  dedupeRemoteItems,
  mergeRemoteItems,
  requestPaneSnapshot,
} from './remoteQueries';

describe('remote query snapshots', () => {
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
    const expected = [{ id: 'p-2', title: 'two' }] as PaneInfo[];
    listener({ type: 'panes', workspaceId: 'ws-2', panes: expected });
    await expect(pending).resolves.toEqual(expected);
    expect(link.listPanes).toHaveBeenCalledOnce();
    expect(stop).toHaveBeenCalledOnce();
  });
});
