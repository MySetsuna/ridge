/**
 * Keep roster requests scoped to the active Remote workspace.
 *
 * An empty workspace id means the shell has not selected a workspace yet;
 * preserve the transport's optional-argument semantics in that case.
 */
export function normalizeTeamRosterWorkspaceId(workspaceId: string): string | undefined {
  return workspaceId || undefined;
}
