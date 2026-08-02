import type { PaneInfo } from '@ridge/remote';

/**
 * Keep roster requests scoped to the active Remote workspace.
 *
 * An empty workspace id means the shell has not selected a workspace yet;
 * preserve the transport's optional-argument semantics in that case.
 */
export function normalizeTeamRosterWorkspaceId(workspaceId: string): string | undefined {
  return workspaceId || undefined;
}

/**
 * Stable identity for a Remote Agent roster view.
 *
 * Pane titles are deliberately excluded: they change frequently and do not
 * alter the CWD/session mapping that the roster renders.  Workspace, pane id,
 * and CWD changes must create a new view scope so an older response cannot
 * repaint the new workspace.
 */
export function teamRosterScopeKey(
  sessionId: number,
  workspaceId: string,
  panes: readonly Pick<PaneInfo, 'id' | 'cwd'>[],
): string {
  const paneScope = panes
    .map((pane) => `${pane.id}\u0000${pane.cwd ?? ''}`)
    .sort()
    .join('\u0001');
  return `${sessionId}\u0000${workspaceId}\u0000${paneScope}`;
}

/** Abort + generation fence shared by roster refresh triggers. */
export function createTeamRosterScopeGuard() {
  let generation = 0;
  let active: AbortController | null = null;
  return {
    begin() {
      active?.abort();
      const controller = new AbortController();
      active = controller;
      return { generation: ++generation, signal: controller.signal };
    },
    invalidate() {
      active?.abort();
      active = null;
      generation += 1;
    },
    isCurrent(candidate: number): boolean {
      return candidate === generation && !active?.signal.aborted;
    },
  };
}
