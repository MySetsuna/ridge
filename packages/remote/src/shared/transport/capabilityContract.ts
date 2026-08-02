/**
 * Controller-facing minimum methods behind each coarse Remote capability.
 *
 * This is deliberately not a second full RPC inventory. It records only the
 * methods the shared controller UI needs before it may expose a capability.
 * Security admission remains canonical in ridge-core's REMOTE_ALLOWLIST.
 */
export const REMOTE_CAPABILITY_METHODS = {
  pane: ['get_pane_layout', 'write_to_pty', 'resize_pane'],
  invoke: [],
  fs: ['get_file_tree', 'read_file'],
  git: ['get_scm_status', 'get_git_info_with_cwd', 'git_diff_file'],
  search: ['text_search'],
  workspace: ['list_workspaces', 'get_active_workspace_id', 'get_workspace_snapshot'],
  theme: ['get_theme_data'],
  teammate: [
    'get_teammate_topology',
    'list_hitl_pending',
    'list_hitl_audit_remote',
    'resolve_hitl_remote',
    'get_orchestration_health',
    'read_agent_recent_replies',
    'set_teammate_groups',
    'resume_agent_session',
  ],
} as const;

export type RemoteCapability = keyof typeof REMOTE_CAPABILITY_METHODS;
export type RemotePanel = 'files' | 'git' | 'search' | 'team';

export const REMOTE_PANEL_CAPABILITY: Readonly<Record<RemotePanel, RemoteCapability>> = {
  files: 'fs',
  git: 'git',
  search: 'search',
  team: 'teammate',
};

export function getRemotePanelAvailability(
  hasCapability: (capability: RemoteCapability) => boolean,
): Readonly<Record<RemotePanel, boolean>> {
  return {
    files: hasCapability(REMOTE_PANEL_CAPABILITY.files),
    git: hasCapability(REMOTE_PANEL_CAPABILITY.git),
    search: hasCapability(REMOTE_PANEL_CAPABILITY.search),
    team: hasCapability(REMOTE_PANEL_CAPABILITY.team),
  };
}
