import type {
  AgentHistoryReply,
  HitlPendingItem,
  PaneInfo,
  RemoteLink,
  RemotePanel,
  TeammateTopology,
  OrchestrationHealth,
  WorkspaceInfo,
  WsMessage,
} from '@ridge/remote';

const sessionIds = new WeakMap<object, number>();
let nextSessionId = 1;

export interface RemoteSidebarScope {
  workspaceId?: string;
  paneId?: string;
  branch?: string;
}

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
  /** Agent sidebar data is scoped by remote session + active workspace. */
  teamRoster: (sessionId: number, workspaceId?: string) =>
    ['remote', sessionId, 'team', workspaceId ?? ''] as const,
  /** Agent history is host-wide; CWD grouping happens in the sidebar. */
  agentHistory: (sessionId: number, limit = 24) =>
    ['remote', sessionId, 'team', 'history', limit] as const,
  /**
   * Sidebar reads use the same TanStack Query cache as workspace/pane reads.
   * Keep cwd and target in every key: two panes can point at the same path
   * while still belonging to different remote sessions.
   */
  sidebarFiles: (sessionId: number, cwd: string, target: string, depth = 1, scope?: RemoteSidebarScope) =>
    ['remote', sessionId, 'sidebar', scopePart(scope?.workspaceId), scopePart(scope?.paneId), scopePart(scope?.branch), 'files', normalizeRemotePath(cwd), normalizeRemotePath(target), depth] as const,
  sidebarGit: (sessionId: number, cwd: string, scope?: RemoteSidebarScope) =>
    ['remote', sessionId, 'sidebar', scopePart(scope?.workspaceId), scopePart(scope?.paneId), scopePart(scope?.branch), 'git', normalizeRemotePath(cwd)] as const,
  sidebarSearch: (sessionId: number, cwd: string, query: string, scope?: RemoteSidebarScope) =>
    ['remote', sessionId, 'sidebar', scopePart(scope?.workspaceId), scopePart(scope?.paneId), scopePart(scope?.branch), 'search', normalizeRemotePath(cwd), query] as const,
  sidebarFile: (sessionId: number, cwd: string, path: string, scope?: RemoteSidebarScope) =>
    ['remote', sessionId, 'sidebar', scopePart(scope?.workspaceId), scopePart(scope?.paneId), scopePart(scope?.branch), 'file', normalizeRemotePath(cwd), normalizeRemotePath(path)] as const,
  sidebarDiff: (sessionId: number, cwd: string, path: string, scope?: RemoteSidebarScope) =>
    ['remote', sessionId, 'sidebar', scopePart(scope?.workspaceId), scopePart(scope?.paneId), scopePart(scope?.branch), 'diff', normalizeRemotePath(cwd), normalizeRemotePath(path)] as const,
};

export const remoteSidebarQueryPrefix = (sessionId: number, scope?: RemoteSidebarScope) =>
  scope
    ? ['remote', sessionId, 'sidebar', scopePart(scope.workspaceId), scopePart(scope.paneId), scopePart(scope.branch)] as const
    : ['remote', sessionId, 'sidebar'] as const;

/** Stable key identity for Windows/Unix paths without changing the RPC path. */
export function normalizeRemotePath(value: string): string {
  const normalized = value.replaceAll('\\', '/').replace(/\/+$/, '') || '/';
  return /^[A-Za-z]:\//.test(normalized) ? normalized.toLowerCase() : normalized;
}

function scopePart(value: string | undefined): string {
  return value?.trim() ?? '';
}

/**
 * Sidebar cache window. QueryClient still owns eviction and single-flight;
 * this explicit stale time is what prevents a tab remount from reloading the
 * same directory/Git snapshot on every open.
 */
export const REMOTE_SIDEBAR_STALE_TIME_MS = 30_000;
export const REMOTE_QUERY_TIMEOUT_MS = 15_000;

export interface RemoteQueryClientLike {
  fetchQuery<T>(options: {
    queryKey: readonly unknown[];
    queryFn: (context?: { signal?: AbortSignal }) => Promise<T>;
    staleTime?: number;
  }): Promise<T>;
  invalidateQueries?: (options: { queryKey: readonly unknown[] }) => Promise<unknown> | unknown;
}

export function fetchRemoteQuery<T>(
  queryClient: RemoteQueryClientLike | undefined,
  queryKey: readonly unknown[],
  queryFn: (context?: { signal?: AbortSignal }) => Promise<T>,
  staleTime = REMOTE_SIDEBAR_STALE_TIME_MS,
  observerSignal?: AbortSignal,
): Promise<T> {
  const work = queryClient
    ? queryClient.fetchQuery({ queryKey, queryFn, staleTime })
    : queryFn({ signal: observerSignal });
  // A component leaving the view must stop observing its old result. When a
  // QueryClient is present, do not feed this per-observer signal into the
  // shared query function: cancelling one observer must not abort another
  // consumer of the same cached request.
  return observerSignal ? abortable(work, [observerSignal]) : work;
}

export interface RemoteTeamRosterSnapshot {
  topology: TeammateTopology;
  pending: HitlPendingItem[];
  health: OrchestrationHealth;
}

/**
 * Query-backed Agent roster snapshot. Keeping this in the shared query layer
 * makes sidebar remounts single-flight and lets the QueryClient retain the
 * last successful snapshot while the drawer is closed.
 */
export function fetchRemoteTeamRoster(
  link: RemoteLink,
  queryClient: RemoteQueryClientLike | undefined,
  sessionId: number,
  workspaceId?: string,
  signal?: AbortSignal,
): Promise<RemoteTeamRosterSnapshot> {
  return fetchRemoteQuery(
    queryClient,
    remoteQueryKeys.teamRoster(sessionId, workspaceId),
    (context) => abortable(
      Promise.all([
        link.getTeammateTopology(workspaceId),
        link.listHitlPending(),
        link.getOrchestrationHealth().catch(() => ({ suspendedAgents: 0, pendingHitl: 0 })),
      ]).then(([topology, pending, health]) => ({ topology, pending, health })),
      [signal, context?.signal],
    ),
    REMOTE_SIDEBAR_STALE_TIME_MS,
  );
}

/** Query-backed Agent history; callers may apply a local capability breaker. */
export function fetchRemoteAgentHistory(
  link: RemoteLink,
  queryClient: RemoteQueryClientLike | undefined,
  sessionId: number,
  limit = 24,
  signal?: AbortSignal,
): Promise<AgentHistoryReply[]> {
  return fetchRemoteQuery(
    queryClient,
    remoteQueryKeys.agentHistory(sessionId, limit),
    (context) => abortable(link.listAgentHistory(limit), [signal, context?.signal]),
    REMOTE_SIDEBAR_STALE_TIME_MS,
  );
}

/**
 * QueryClient cancellation must stop the caller from observing a late remote
 * snapshot even when the legacy RemoteLink method has no signal parameter.
 * The transport promise may still finish its bounded RPC timeout, but it can
 * no longer commit data into the new scope.
 */
function abortable<T>(work: Promise<T>, signals: readonly (AbortSignal | undefined)[]): Promise<T> {
  const active = signals.filter((value): value is AbortSignal => value !== undefined);
  const aborted = active.find((signal) => signal.aborted);
  if (aborted) return Promise.reject(aborted.reason ?? new DOMException('Aborted', 'AbortError'));
  if (active.length === 0) return work;
  return new Promise<T>((resolve, reject) => {
    let settled = false;
    const cleanup = () => active.forEach((signal) => signal.removeEventListener('abort', onAbort));
    const finish = <V>(fn: (value: V) => void, value: V) => {
      if (settled) return;
      settled = true;
      cleanup();
      fn(value);
    };
    const onAbort = () => {
      const signal = active.find((candidate) => candidate.aborted);
      finish(reject, signal?.reason ?? new DOMException('Aborted', 'AbortError'));
    };
    active.forEach((signal) => signal.addEventListener('abort', onAbort, { once: true }));
    work.then(
      (value) => finish(resolve, value),
      (error) => finish(reject, error),
    );
  });
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
  timeoutMs = REMOTE_QUERY_TIMEOUT_MS,
): Promise<PaneInfo[]> {
  return new Promise((resolve, reject) => {
    let stop = () => {};
    let timer: ReturnType<typeof setTimeout> | undefined;
    let settled = false;
    const cleanup = () => {
      stop();
      signal?.removeEventListener('abort', abort);
      if (timer !== undefined) clearTimeout(timer);
    };
    const finish = (fn: () => void) => {
      if (settled) return;
      settled = true;
      cleanup();
      fn();
    };
    const abort = () => {
      finish(() => reject(signal?.reason ?? new DOMException('Aborted', 'AbortError')));
    };
    stop = link.onMessage((message: WsMessage) => {
      if (message.type !== 'panes') return;
      if (message.workspaceId !== workspaceId) return;
      finish(() => resolve(dedupeRemoteItems(message.panes)));
    });
    if (signal?.aborted) {
      abort();
      return;
    }
    signal?.addEventListener('abort', abort, { once: true });
    timer = setTimeout(() => finish(() => reject(new Error(`list panes timed out after ${timeoutMs}ms`))), timeoutMs);
    try {
      link.listPanes();
    } catch (error) {
      finish(() => reject(error));
    }
  });
}

export function requestWorkspaceSnapshot(
  link: RemoteLink,
  signal?: AbortSignal,
  timeoutMs = REMOTE_QUERY_TIMEOUT_MS,
): Promise<WorkspaceInfo[]> {
  return new Promise((resolve, reject) => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    let settled = false;
    const cleanup = () => {
      signal?.removeEventListener('abort', abort);
      if (timer !== undefined) clearTimeout(timer);
    };
    const finish = (fn: () => void) => {
      if (settled) return;
      settled = true;
      cleanup();
      fn();
    };
    const abort = () => {
      finish(() => reject(signal?.reason ?? new DOMException('Aborted', 'AbortError')));
    };
    if (signal?.aborted) {
      abort();
      return;
    }
    signal?.addEventListener('abort', abort, { once: true });
    timer = setTimeout(() => finish(() => reject(new Error(`list workspaces timed out after ${timeoutMs}ms`))), timeoutMs);
    Promise.resolve()
      .then(() => link.listWorkspaces())
      .then(
        (result) => finish(() => resolve(dedupeRemoteItems(result.workspaces ?? []))),
        (error) => finish(() => reject(error)),
      );
  });
}

export async function confirmedWorkspaceTarget(
  switchWorkspace: (workspaceId: string) => Promise<boolean>,
  workspaceId: string,
  paneId: string | null,
): Promise<{ workspaceId: string; paneId: string | null } | null> {
  return await switchWorkspace(workspaceId) ? { workspaceId, paneId } : null;
}
