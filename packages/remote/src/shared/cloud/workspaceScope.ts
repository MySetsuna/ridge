import { isDesktopPrivileged } from '../transport/protocolAdmission';

export interface WorkspaceScopeAssertion {
  grantId: string;
  granteeUserId: string;
  ownerUserId: string;
  deviceName: string;
  workspaceId: string;
  role: 'operator';
  delegable: false;
}

export interface WorkspaceAccess extends WorkspaceScopeAssertion {
  paneIds: Set<string>;
  roots: string[];
}

export type WorkspaceInvokePlan =
  | { kind: 'deny'; reason: string }
  | { kind: 'result'; value: unknown }
  | { kind: 'invoke'; method: string; params: Record<string, unknown> };

const NEVER_SHARED = new Set([
  'create_workspace',
  'close_workspace',
  'reorder_workspaces',
  'save_workspace',
  'list_saved_workspaces',
  'delete_saved_workspace',
  'rename_saved_workspace',
  'list_workspace_save_info',
  'delete_workspace_file',
  'get_default_workspace_save_dir',
  'list_saved_workspace_files',
  'save_workspace_to_file',
  'open_workspace_from_file',
  'get_restore_set',
  'list_recent_workspaces',
  'clear_recent_workspaces',
  'get_last_opened_workspace_path',
  'get_startup_context',
  'browse_directory',
  // Explicit second-hop/host controls. Most are already absent from the remote
  // allowlist; pin them here so a future allowlist expansion cannot delegate them.
  'connect_host',
  'disconnect_host',
  'start_remote_server',
  'stop_remote_server',
  'get_remote_info',
  'get_remote_totp',
  'set_cloud_remote_active',
]);

const PANE_KEYS = new Set(['paneId', 'targetPaneId', 'sourcePaneId']);
const PATH_KEYS = new Set([
  'path',
  'cwd',
  'root',
  'repoRoot',
  'sourcePath',
  'destinationPath',
  'oldPath',
  'newPath',
  'filePath',
]);

function normalizedAbsolutePath(raw: string): string | null {
  const value = raw.trim().replace(/\\/g, '/');
  const drive = /^([A-Za-z]:)(\/.*)?$/.exec(value);
  const unix = value.startsWith('/');
  if (!drive && !unix) return null;
  const prefix = drive ? `${drive[1].toLowerCase()}/` : '/';
  const body = drive ? (drive[2] ?? '/').slice(1) : value.slice(1);
  const out: string[] = [];
  for (const part of body.split('/')) {
    if (!part || part === '.') continue;
    if (part === '..') {
      if (out.length === 0) return null;
      out.pop();
    } else {
      out.push(part);
    }
  }
  const joined = `${prefix}${out.join('/')}`;
  return drive ? joined.toLowerCase() : joined;
}

export function pathWithinRoots(path: string, roots: readonly string[]): boolean {
  const candidate = normalizedAbsolutePath(path);
  if (!candidate) return false;
  return roots.some((rawRoot) => {
    const root = normalizedAbsolutePath(rawRoot);
    return root !== null && (candidate === root || candidate.startsWith(`${root}/`));
  });
}

export function collectPaneIds(layout: unknown): Set<string> {
  const ids = new Set<string>();
  const visit = (value: unknown): void => {
    if (!value || typeof value !== 'object') return;
    if (Array.isArray(value)) {
      for (const child of value) visit(child);
      return;
    }
    const record = value as Record<string, unknown>;
    if (typeof record.id === 'string' && record.type !== 'split') ids.add(record.id);
    if (typeof record.paneId === 'string') ids.add(record.paneId);
    for (const child of Object.values(record)) visit(child);
  };
  visit(layout);
  return ids;
}

function containsForeignResource(params: Record<string, unknown>, access: WorkspaceAccess): string | null {
  const visit = (value: unknown, key = ''): string | null => {
    if (typeof value === 'string') {
      if (key === 'workspaceId' && value !== access.workspaceId) return 'workspace mismatch';
      if (PANE_KEYS.has(key) && !access.paneIds.has(value)) return 'pane outside shared workspace';
      if (PATH_KEYS.has(key) && !pathWithinRoots(value, access.roots)) {
        return 'path outside shared workspace';
      }
      return null;
    }
    if (Array.isArray(value)) {
      for (const item of value) {
        const hit = visit(item, key);
        if (hit) return hit;
      }
      return null;
    }
    if (value && typeof value === 'object') {
      for (const [childKey, child] of Object.entries(value as Record<string, unknown>)) {
        const hit = visit(child, childKey);
        if (hit) return hit;
      }
    }
    return null;
  };
  return visit(params);
}

export function planWorkspaceInvoke(
  method: string,
  rawParams: unknown,
  access: WorkspaceAccess,
): WorkspaceInvokePlan {
  if (access.role !== 'operator' || access.delegable !== false) {
    return { kind: 'deny', reason: 'unsupported workspace-share role' };
  }
  if (NEVER_SHARED.has(method) || isDesktopPrivileged(method)) {
    return { kind: 'deny', reason: 'second hop or host-global method' };
  }

  const params =
    rawParams && typeof rawParams === 'object' && !Array.isArray(rawParams)
      ? { ...(rawParams as Record<string, unknown>) }
      : {};

  if (method === 'get_active_workspace_id') return { kind: 'result', value: access.workspaceId };
  if (method === 'switch_workspace') {
    return params.workspaceId === access.workspaceId
      ? { kind: 'result', value: null }
      : { kind: 'deny', reason: 'workspace mismatch' };
  }
  if (method === 'get_current_project') {
    return { kind: 'result', value: access.roots[0] ?? null };
  }
  if (method === 'get_pane_layout') {
    return { kind: 'invoke', method: 'get_pane_layout_for', params: { workspaceId: access.workspaceId } };
  }

  const foreign = containsForeignResource(params, access);
  if (foreign) return { kind: 'deny', reason: foreign };

  // Workspace-scoped commands must not inherit the host's currently active workspace.
  if (
    method.includes('workspace') ||
    method.startsWith('get_teammate_') ||
    method.startsWith('list_hitl_') ||
    method === 'resolve_hitl_remote' ||
    method === 'get_orchestration_health' ||
    method === 'read_agent_recent_replies' ||
    method === 'set_teammate_groups' ||
    method === 'resume_agent_session' ||
    method === 'register_teammate_agent' ||
    method === 'release_teammate_agent'
  ) {
    params.workspaceId = access.workspaceId;
  }
  return { kind: 'invoke', method, params };
}

export function filterWorkspaceResult(
  method: string,
  result: unknown,
  workspaceId: string,
): unknown {
  if (method !== 'list_workspaces') return result;
  if (Array.isArray(result)) {
    return result.filter(
      (entry) =>
        !!entry &&
        typeof entry === 'object' &&
        (entry as Record<string, unknown>).id === workspaceId,
    );
  }
  return result;
}
