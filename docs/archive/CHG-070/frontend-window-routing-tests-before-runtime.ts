// src/lib/stores/paneTree.test.ts


  it('does not switch or load a workspace owned by another window', async () => {
    const { invoke } = await import('@tauri-apps/api/core');
    const invokeMock = invoke as ReturnType<typeof vi.fn>;
    invokeMock.mockReset();
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'claim_workspace_window') {
        return { claimed: false, ownerWindowLabel: 'main' };
      }
      throw new Error(`unexpected command: ${cmd}`);
    });

    await expect(paneTreeModule.switchWorkspace('owned-ws')).resolves.toBe(false);
    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith('claim_workspace_window', {
      workspaceId: 'owned-ws',
    });
  });

// src/lib/stores/paneTree.coverage.test.ts


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

// src/lib/stores/paneTree.coverage.test.ts


  it('rejects a workspace claim without issuing a switch command', async () => {
    core.invoke.mockImplementation(async (command: string) => {
      if (command === 'claim_workspace_window') return { claimed: false, ownerWindowLabel: 'other' };
      return undefined;
    });
    await expect(paneTree.switchWorkspace('ws-new')).resolves.toBe(false);
    expect(core.invoke).toHaveBeenCalledWith('claim_workspace_window', expect.anything());
    expect(core.invoke).not.toHaveBeenCalledWith('switch_window_workspace', expect.anything());
  });
