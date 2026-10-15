

const serviceClient = import.meta.env.RIDGE_WEB_REMOTE === true || import.meta.env.RIDGE_NATIVE_RUNTIME === true;

const activeWorkspaceCommand = serviceClient
  ? 'get_active_workspace_id'
  : 'get_window_active_workspace_id';

const paneLayoutCommand = serviceClient ? 'get_pane_layout' : 'get_window_pane_layout';

const switchWorkspaceCommand = serviceClient
  ? 'switch_workspace'
  : 'switch_window_workspace';


type WorkspaceWindowClaimResult = {
  claimed: boolean;
  ownerWindowLabel: string;
};


async function claimWorkspaceForThisWindow(workspaceId: string): Promise<boolean> {
  if (serviceClient) return true;
  const result = await invoke<WorkspaceWindowClaimResult>(
    'claim_workspace_window',
    { workspaceId }
  );
  // Older hosts and lightweight test shims may not expose this desktop-only
  // command yet; their undefined result preserves the former single-window path.
  return result?.claimed ?? true;
}


export async function refreshWorkspaces(options: RefreshWorkspacesOptions = {}) {
  if (!isTauri()) return;
  try {
    let list = await invoke<
      { id: string; index: number; name?: string; displaySeq: number }[]
    >('list_workspaces');
    let active: string;
    if (options.workspaceId) {
      active = options.workspaceId;
    } else if (serviceClient) {
      active = await invoke<string>(activeWorkspaceCommand);
    } else if (options.acquireWindow !== false) {
      active = await invoke<string>('acquire_window_workspace');
    } else {
      active = get(activeWorkspaceId) || list[0]?.id || '';
    }
    if (!active) {
      // A close can briefly leave the backend with an empty list while the
      // replacement workspace event is still in flight.  Keep the list
      // coherent and let the existing startup/ensure path create or select
      // the replacement instead of rejecting the whole refresh.
      workspacesList.set(list);
      await refreshWorkspaceSaveInfo();
      return;
    }
    // The atomic desktop acquisition creates a workspace only when every
    // existing one is owned by another window. Refresh the snapshot exactly
    // once in that uncommon path so the new tab is present immediately.
    if (!serviceClient && !list.some((workspace) => workspace.id === active)) {
      list = await invoke<
        { id: string; index: number; name?: string; displaySeq: number }[]
      >('list_workspaces');
    }
    const layout = await invoke<PaneNode>('get_pane_layout_for', {
      workspaceId: active,
    });
    workspacesList.set(list);
    setActiveTree(active, layout);
    activeWorkspaceId.set(active);
    reconcileActivePaneId(layout, active);
    // §4a keep-alive 懒挂载后（iter-62）不再预取所有工作区 layout：+page.svelte
    // 只挂访问过的工作区，未访问的树取来也没人用，而每次刷新按 tab 数扇出 N 次
    // `get_pane_layout_for` 正是「tab 越多越卡」的一份贡献。首次切换由
    // `switchWorkspace` 自行取该区 layout 并写缓存。
    const cwds = extractCwdsFromLayout(layout, active);
    paneCwdStore.update((store) => mergePaneCwds(store, cwds));
    await setupPaneCwdListeners(active);
    // Save info changes on workspace add/remove/rename; keep UI badges accurate.
    await refreshWorkspaceSaveInfo();
  } catch (e) {
    console.error('refreshWorkspaces', e);
    reportDevIssue({
      title: 'Workspace refresh failed',
      message: String(e),
      stack: e instanceof Error ? e.stack : undefined,
    });
    throw e;
  }
}


export async function createWorkspace() {
  if (!isTauri()) return;
  try {
    const created = await invoke<string>(
      serviceClient ? 'create_workspace' : 'create_workspace_for_window'
    );
    if (serviceClient) {
      if (!created || !await switchWorkspace(created)) throw new Error('Workspace created but caller selection failed');
      await refreshWorkspaces({ workspaceId: created, acquireWindow: false });
    } else {
      await refreshWorkspaces();
    }
  } catch (e) {
    console.error('createWorkspace', e);
    reportDevIssue({
      title: 'Workspace create failed',
      message: String(e),
      stack: e instanceof Error ? e.stack : undefined,
    });
    throw e;
  }
}