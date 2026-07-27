import { describe, expect, it, vi } from 'vitest';
import { loadHostForest, type HostForestSource } from './hostForest';

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
});
