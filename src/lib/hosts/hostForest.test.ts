import { describe, expect, it, vi } from 'vitest';
import {
  hostTopologyErrorKind,
  loadHostForest,
  retainHostForest,
  type HostForestResult,
  type HostForestSource,
} from './hostForest';

function source(
  hostId: string,
  workspaces: Array<{ id: string; name: string; panes: Array<{ id: string; cwd: string }> }>,
): HostForestSource {
  return {
    hostId,
    link: {
      listWorkspaces: vi.fn(async () => ({
        workspaces: workspaces.map((w, index) => ({
          id: w.id,
          name: w.name,
          active: index === 0,
        })),
      })),
      listWorkspacePanes: vi.fn(async (workspaceId: string) =>
        workspaces
          .find((w) => w.id === workspaceId)!
          .panes.map((pane) => ({ ...pane, title: pane.id })),
      ),
    },
  };
}

describe('loadHostForest', () => {
  it('keeps two hosts and their workspace/pane trees isolated', async () => {
    const a = source('host-a', [
      { id: 'wa', name: 'alpha', panes: [{ id: 'pa', cwd: 'C:\\alpha' }] },
    ]);
    const b = source('host-b', [
      { id: 'wb', name: 'beta', panes: [{ id: 'pb', cwd: '/srv/beta' }] },
    ]);

    const forest = await loadHostForest([a, b]);

    expect(forest).toEqual([
      {
        hostId: 'host-a',
        workspaces: [{
          id: 'wa',
          name: 'alpha',
          active: true,
          panes: [{ id: 'pa', title: 'pa', cwd: 'C:\\alpha', isAgent: false }],
        }],
      },
      {
        hostId: 'host-b',
        workspaces: [{
          id: 'wb',
          name: 'beta',
          active: true,
          panes: [{ id: 'pb', title: 'pb', cwd: '/srv/beta', isAgent: false }],
        }],
      },
    ]);
  });

  it('does not let one failed host erase another host tree', async () => {
    const good = source('good', [
      { id: 'w', name: 'ok', panes: [{ id: 'p', cwd: '/ok' }] },
    ]);
    const bad: HostForestSource = {
      hostId: 'bad',
      link: {
        listWorkspaces: vi.fn(async () => { throw new Error('offline'); }),
        listWorkspacePanes: vi.fn(),
      },
    };

    expect(await loadHostForest([bad, good])).toEqual([
      { hostId: 'bad', workspaces: [], error: 'offline' },
      {
        hostId: 'good',
        workspaces: [{
          id: 'w',
          name: 'ok',
          active: true,
          panes: [{ id: 'p', title: 'p', cwd: '/ok', isAgent: false }],
        }],
      },
    ]);
  });

  it('retains the last successful tree when a refresh fails', () => {
    const previous: HostForestResult = {
      hostId: 'host-a',
      workspaces: [{ id: 'w', name: 'kept', active: true, panes: [] }],
    };
    expect(retainHostForest(previous, {
      hostId: 'host-a',
      workspaces: [],
      error: 'list_workspaces timeout',
    })).toEqual({
      hostId: 'host-a',
      workspaces: previous.workspaces,
      error: 'list_workspaces timeout',
    });
    expect(retainHostForest({
      ...previous,
      error: 'first timeout',
    }, {
      hostId: 'host-a',
      workspaces: [],
      error: 'second timeout',
    }).workspaces).toEqual(previous.workspaces);
  });

  it('cancels a pending host refresh without waiting for its transport', async () => {
    const controller = new AbortController();
    const pending: HostForestSource = {
      hostId: 'slow',
      signal: controller.signal,
      link: {
        listWorkspaces: vi.fn(
          () => new Promise<{ workspaces: [] }>(() => {}),
        ),
        listWorkspacePanes: vi.fn(),
      },
    };
    const result = loadHostForest([pending]);
    controller.abort();
    await expect(result).resolves.toEqual([
      { hostId: 'slow', workspaces: [], error: '请求已取消' },
    ]);
  });

  it('separates authentication failures from retryable transport failures', () => {
    expect(hostTopologyErrorKind('TOTP 验证失败')).toBe('auth');
    expect(hostTopologyErrorKind('list_workspaces timeout')).toBe('retryable');
  });
});
