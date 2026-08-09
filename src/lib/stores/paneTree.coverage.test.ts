import { beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

const core = vi.hoisted(() => ({
  invoke: vi.fn(),
  isTauri: vi.fn(() => true),
}));
const event = vi.hoisted(() => ({ listen: vi.fn(async () => vi.fn()) }));
const manager = vi.hoisted(() => ({ fitPaneNow: vi.fn(), detach: vi.fn(), forceFullRedrawFor: vi.fn() }));

vi.mock('@tauri-apps/api/core', () => core);
vi.mock('@tauri-apps/api/event', () => event);
vi.mock('@ridge/remote/shared/terminal/manager', () => ({ TerminalManager: { instance: () => manager } }));
vi.mock('@ridge/remote/shared/terminal/ptyBridge', () => ({
  ensurePtyBridge: vi.fn(async () => {}),
  teardownPtyBridge: vi.fn(),
}));

const paneTree = await import('./paneTree');

const layout = {
  type: 'leaf' as const,
  id: 'pane-a',
  cwd: '/repo',
};

beforeEach(() => {
  core.invoke.mockReset();
  core.isTauri.mockReset().mockReturnValue(true);
  event.listen.mockReset().mockResolvedValue(vi.fn());
  paneTree.activeWorkspaceId.set('');
  paneTree.activePaneId.set('');
  paneTree.paneTreeStore.set(layout);
  paneTree.workspacePaneTrees.set(new Map());
  paneTree.workspacesList.set([]);
  paneTree.workspaceSaveInfoStore.set({});
  paneTree.workspaceNames.set({});
});

function mockRefreshCommands(active = 'ws-a') {
  core.invoke.mockImplementation(async (command: string) => {
    if (command === 'list_workspaces') return [{ id: 'ws-a', index: 0, displaySeq: 0 }];
    if (command === 'acquire_window_workspace' || command === 'get_active_workspace_id') return active;
    if (command === 'get_pane_layout_for' || command === 'get_window_pane_layout') return layout;
    if (command === 'list_workspace_save_info') return [];
    return undefined;
  });
}

describe('paneTree workspace mutation branches', () => {
  it('refreshes an explicitly selected workspace and keeps cwd/listener projections aligned', async () => {
    mockRefreshCommands();
    await paneTree.refreshWorkspaces({ workspaceId: 'ws-a', acquireWindow: false });
    expect(get(paneTree.activeWorkspaceId)).toBe('ws-a');
    expect(get(paneTree.paneTreeStore)).toEqual(layout);
    expect(get(paneTree.workspacesList)).toEqual([{ id: 'ws-a', index: 0, displaySeq: 0 }]);
    expect(get(paneTree.paneCwdStore)['ws-a:pane-a']).toBe('/repo');
    expect(event.listen).toHaveBeenCalledWith('pane-cwd-changed-ws-a-pane-a', expect.any(Function));
  });

  it('handles empty acquisition and desktop acquisition that creates a missing workspace', async () => {
    let listCalls = 0;
    core.invoke.mockImplementation(async (command: string) => {
      if (command === 'list_workspaces') {
        listCalls += 1;
        return listCalls === 1
          ? [{ id: 'old', index: 0, displaySeq: 0 }]
          : [{ id: 'new', index: 0, displaySeq: 0 }];
      }
      if (command === 'acquire_window_workspace') return 'new';
      if (command === 'get_pane_layout_for') return layout;
      if (command === 'list_workspace_save_info') return [];
      return undefined;
    });
    await paneTree.refreshWorkspaces();
    expect(listCalls).toBe(2);
    expect(get(paneTree.activeWorkspaceId)).toBe('new');

    paneTree.activeWorkspaceId.set('');
    core.invoke.mockImplementation(async (command: string) => {
      if (command === 'list_workspaces') return [{ id: 'waiting', index: 0, displaySeq: 0 }];
      if (command === 'acquire_window_workspace') return '';
      if (command === 'list_workspace_save_info') return [];
      return undefined;
    });
    await paneTree.refreshWorkspaces();
    expect(get(paneTree.workspacesList)).toEqual([{ id: 'waiting', index: 0, displaySeq: 0 }]);
  });

  it('rethrows refresh/create failures and does not hide backend errors', async () => {
    core.invoke.mockRejectedValue(new Error('refresh failed'));
    await expect(paneTree.refreshWorkspaces()).rejects.toThrow('refresh failed');
    core.invoke.mockRejectedValue(new Error('create failed'));
    await expect(paneTree.createWorkspace()).rejects.toThrow('create failed');
  });

  it('creates a workspace and refreshes the authoritative snapshot', async () => {
    const commands: string[] = [];
    mockRefreshCommands();
    core.invoke.mockImplementation(async (command: string) => {
      commands.push(command);
      if (command === 'create_workspace_for_window') return 'ws-created';
      if (command === 'list_workspaces') return [{ id: 'ws-a', index: 0, displaySeq: 0 }];
      if (command === 'acquire_window_workspace') return 'ws-a';
      if (command === 'get_pane_layout_for') return layout;
      if (command === 'list_workspace_save_info') return [];
      return undefined;
    });
    await paneTree.createWorkspace();
    expect(commands).toEqual(expect.arrayContaining(['create_workspace_for_window', 'list_workspaces']));
  });

  it('rolls back an optimistic workspace switch when the backend rejects it', async () => {
    paneTree.activeWorkspaceId.set('ws-old');
    paneTree.paneTreeStore.set({ type: 'leaf', id: 'old-pane' });
    paneTree.workspacePaneTrees.set(new Map([
      ['ws-old', { type: 'leaf', id: 'old-pane' }],
      ['ws-new', { type: 'leaf', id: 'new-pane' }],
    ]));
    core.invoke.mockImplementation(async (command: string) => {
      if (command === 'claim_workspace_window') return { claimed: true, ownerWindowLabel: 'main' };
      if (command === 'switch_window_workspace') throw new Error('switch failed');
      return undefined;
    });
    await expect(paneTree.switchWorkspace('ws-new')).rejects.toThrow('switch failed');
    expect(get(paneTree.activeWorkspaceId)).toBe('ws-old');
    expect(get(paneTree.paneTreeStore)).toEqual({ type: 'leaf', id: 'old-pane' });
  });

  it('rejects a workspace claim without issuing a switch command', async () => {
    core.invoke.mockImplementation(async (command: string) => {
      if (command === 'claim_workspace_window') return { claimed: false, ownerWindowLabel: 'other' };
      return undefined;
    });
    await expect(paneTree.switchWorkspace('ws-new')).resolves.toBe(false);
    expect(core.invoke).toHaveBeenCalledWith('claim_workspace_window', expect.anything());
    expect(core.invoke).not.toHaveBeenCalledWith('switch_window_workspace', expect.anything());
  });

  it('rolls back workspace ordering and names after mutation failures', async () => {
    paneTree.workspacesList.set([
      { id: 'a', index: 0, displaySeq: 0 },
      { id: 'b', index: 1, displaySeq: 1 },
    ]);
    core.invoke.mockImplementation(async (command: string) => {
      if (command === 'reorder_workspaces') throw new Error('order failed');
      if (command === 'rename_workspace') throw new Error('rename failed');
      return undefined;
    });
    await expect(paneTree.reorderWorkspaces(0, 1)).rejects.toThrow('order failed');
    expect(get(paneTree.workspacesList).map((item) => item.id)).toEqual(['a', 'b']);
    await expect(paneTree.renameWorkspace('a', 'new name')).rejects.toThrow('rename failed');
    expect(get(paneTree.workspaceNames)).toEqual({});
  });

  it('maps saved-workspace commands and preserves the backend-selected id', async () => {
    mockRefreshCommands('ws-opened');
    core.invoke.mockImplementation(async (command: string) => {
      if (command === 'save_workspace_to_file') return 'ws-saved';
      if (command === 'open_workspace_from_file') return 'ws-opened';
      if (command === 'delete_workspace_file' || command === 'delete_saved_workspace_file') return undefined;
      if (command === 'get_default_workspace_save_dir') return 'C:/Ridge';
      if (command === 'list_workspace_save_info') return [];
      if (command === 'list_workspaces') return [{ id: 'ws-opened', index: 0, displaySeq: 0 }];
      if (command === 'get_pane_layout_for') return layout;
      if (command === 'get_active_workspace_id' || command === 'acquire_window_workspace') return 'ws-opened';
      return undefined;
    });
    await expect(paneTree.saveWorkspaceToFile('ws-a', 'Name')).resolves.toBe('ws-saved');
    await expect(paneTree.openWorkspaceFromFile('C:/a.ridge')).resolves.toBe('ws-opened');
    await paneTree.deleteWorkspaceFile('ws-opened');
    await paneTree.deleteSavedWorkspaceFile('C:/a.ridge');
    await expect(paneTree.getDefaultWorkspaceSaveDir()).resolves.toBe('C:/Ridge');
    expect(core.invoke).toHaveBeenCalledWith('open_workspace_from_file', { path: 'C:/a.ridge' });
    expect(core.invoke).toHaveBeenCalledWith('save_workspace_to_file', {
      workspaceId: 'ws-a', name: 'Name', path: null,
    });
  });
});
