import type {
  PaneInfo,
  RemoteLink,
  RemotePanel,
  WorkspaceInfo,
  WsMessage,
} from '@ridge/remote';

const sessionIds = new WeakMap<object, number>();
let nextSessionId = 1;

export function remoteSessionId(link: RemoteLink): number {
  let id = sessionIds.get(link);
  if (id === undefined) {
    id = nextSessionId++;
    sessionIds.set(link, id);
  }
  return id;
}

export const remoteQueryKeys = {
  workspaces: (sessionId: number) => ['remote', sessionId, 'workspaces'] as const,
  panes: (sessionId: number, workspaceId: string) =>
    ['remote', sessionId, 'panes', workspaceId] as const,
  capabilities: (sessionId: number) => ['remote', sessionId, 'capabilities'] as const,
  /**
   * Sidebar reads use the same TanStack Query cache as workspace/pane reads.
   * Keep cwd and target in every key: two panes can point at the same path
   * while still belonging to different remote sessions.
   */
  sidebarFiles: (sessionId: number, cwd: string, target: string, depth = 1) =>
    ['remote', sessionId, 'sidebar', 'files', normalizeRemotePath(cwd), normalizeRemotePath(target), depth] as const,
  sidebarGit: (sessionId: number, cwd: string) =>
    ['remote', sessionId, 'sidebar', 'git', normalizeRemotePath(cwd)] as const,
  sidebarSearch: (sessionId: number, cwd: string, query: string) =>
    ['remote', sessionId, 'sidebar', 'search', normalizeRemotePath(cwd), query] as const,
  sidebarFile: (sessionId: number, cwd: string, path: string) =>
    ['remote', sessionId, 'sidebar', 'file', normalizeRemotePath(cwd), normalizeRemotePath(path)] as const,
  sidebarDiff: (sessionId: number, cwd: string, path: string) =>
    ['remote', sessionId, 'sidebar', 'diff', normalizeRemotePath(cwd), normalizeRemotePath(path)] as const,
};

export const remoteSidebarQueryPrefix = (sessionId: number) =>
  ['remote', sessionId, 'sidebar'] as const;

/** Stable key identity for Windows/Unix paths without changing the RPC path. */
export function normalizeRemotePath(value: string): string {
  const normalized = value.replaceAll('\\', '/').replace(/\/+$/, '') || '/';
  return /^[A-Za-z]:\//.test(normalized) ? normalized.toLowerCase() : normalized;
}

/**
 * Sidebar cache window. QueryClient still owns eviction and single-flight;
 * this explicit stale time is what prevents a tab remount from reloading the
 * same directory/Git snapshot on every open.
 */
export const REMOTE_SIDEBAR_STALE_TIME_MS = 30_000;

export interface RemoteQueryClientLike {
  fetchQuery<T>(options: {
    queryKey: readonly unknown[];
    queryFn: () => Promise<T>;
    staleTime?: number;
  }): Promise<T>;
  invalidateQueries?: (options: { queryKey: readonly unknown[] }) => Promise<unknown> | unknown;
}

export function fetchRemoteQuery<T>(
  queryClient: RemoteQueryClientLike | undefined,
  queryKey: readonly unknown[],
  queryFn: () => Promise<T>,
  staleTime = REMOTE_SIDEBAR_STALE_TIME_MS,
): Promise<T> {
  if (!queryClient) return queryFn();
  return queryClient.fetchQuery({ queryKey, queryFn, staleTime });
}

export type RemoteCapabilities = {
  panels: Readonly<Record<RemotePanel, boolean>>;
  manageWorkspaces: boolean;
  managePanes: boolean;
  theme: boolean;
};

export function dedupeRemoteItems<T extends { id: string }>(items: readonly T[]): T[] {
  const byId = new Map<string, T>();
  for (const item of items) byId.set(item.id, item);
  return [...byId.values()];
}

export function mergeRemoteItems<T extends { id: string }>(
  current: readonly T[] | undefined,
  incoming: readonly T[],
): T[] {
  const byId = new Map((current ?? []).map((item) => [item.id, item]));
  for (const item of incoming) {
    byId.set(item.id, { ...byId.get(item.id), ...item });
  }
  return [...byId.values()];
}

/** One list-panes request → its next canonical snapshot. Pushes remain separately merged. */
export function requestPaneSnapshot(
  link: RemoteLink,
  workspaceId: string,
  signal?: AbortSignal,
): Promise<PaneInfo[]> {
  return new Promise((resolve, reject) => {
    let stop = () => {};
    const abort = () => {
      stop();
      reject(signal?.reason ?? new DOMException('Aborted', 'AbortError'));
    };
    stop = link.onMessage((message: WsMessage) => {
      if (message.type !== 'panes') return;
      if (message.workspaceId !== workspaceId) return;
      stop();
      signal?.removeEventListener('abort', abort);
      resolve(dedupeRemoteItems(message.panes));
    });
    if (signal?.aborted) {
      abort();
      return;
    }
    signal?.addEventListener('abort', abort, { once: true });
    link.listPanes();
  });
}

export async function requestWorkspaceSnapshot(link: RemoteLink): Promise<WorkspaceInfo[]> {
  const result = await link.listWorkspaces();
  return dedupeRemoteItems(result.workspaces ?? []);
}

export async function confirmedWorkspaceTarget(
  switchWorkspace: (workspaceId: string) => Promise<boolean>,
  workspaceId: string,
  paneId: string | null,
): Promise<{ workspaceId: string; paneId: string | null } | null> {
  return await switchWorkspace(workspaceId) ? { workspaceId, paneId } : null;
}
