  // Foreign multi-host view: best-effort detach first (unsubscribe remote;
  // backend kill_pty_if_present also detaches — double-call is safe).
  try {
    const tree = get(paneTreeStore);
    const leaf = findLeafOrigin(tree, paneId);
    if (
      leaf?.type === 'leaf' &&
      leaf.origin &&
      (leaf.origin.kind === 'remote' || leaf.origin.kind === 'rdg')
    ) {
      await invoke('detach_host_session', {
        paneId,
        workspaceId: get(activeWorkspaceId) || null,
      });
    }
  } catch {
    /* local pane or already detached */
  }
