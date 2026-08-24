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
import { trimTrailingSeparators } from '$lib/utils/path';
import { unknownText } from '@ridge/remote/shared/transport/unknownText';

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
  agentHistory: (sessionId: number, limit = 24, offset = 0, query = '') =>
    offset === 0 && !query
      ? ['remote', sessionId, 'team', 'history', limit] as const
      : ['remote', sessionId, 'team', 'history', limit, offset, query] as const,
  /**
   * Sidebar reads use the same TanStack Query cache as workspace/pane reads.
   * Keep cwd and target in every key: two panes can point at the same path
   * while still belonging to different remote sessions.
   */
  sidebarFiles: (sessionId: number, cwd: string, target: string, depth = 1, scope?: RemoteSidebarScope) =>
    ['remote', sessionId, 'sidebar', scopePart(scope?.workspaceId), scopePart(scope?.paneId), scopePart(scope?.branch), 'files', normalizeRemotePath(cwd), normalizeRemotePath(target), depth] as const,
  sidebarGit: (sessionId: number, cwd: string, scope?: RemoteSidebarScope) =>
    ['remote', sessionId, 'sidebar', scopePart(scope?.workspaceId), scopePart(scope?.paneId), scopePart(scope?.branch), 'git', normalizeRemotePath(cwd)] as const,
  sidebarGitGraph: (sessionId: number, cwd: string, scope?: RemoteSidebarScope) =>
    ['remote', sessionId, 'sidebar', scopePart(scope?.workspaceId), scopePart(scope?.paneId), scopePart(scope?.branch), 'git-graph', normalizeRemotePath(cwd)] as const,
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
  const normalized = trimTrailingSeparators(value.replaceAll('\\', '/')) || '/';
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
/** Live roster attention must converge quickly; history remains five-minute. */
export const REMOTE_ROSTER_STALE_TIME_MS = 3_000;
export const REMOTE_QUERY_TIMEOUT_MS = 15_000;

export interface RemoteQueryClientLike {
  fetchQuery<T>(options: {
    queryKey: readonly unknown[];
    queryFn: (context?: { signal?: AbortSignal }) => Promise<T>;
    staleTime?: number;
  }): Promise<T>;
  invalidateQueries?: (options: { queryKey: readonly unknown[] }) => unknown;
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

const EMPTY_TEAM_TOPOLOGY: TeammateTopology = { roster: [], leaderId: null, edges: [] };

function isUnsupportedHostMethod(error: unknown): boolean {
  return /method not supported by kernel host/i.test(error instanceof Error ? error.message : unknownText(error));
}

function emptyOnUnsupported<T>(work: Promise<T>, fallback: T): Promise<T> {
  return work.catch((error) => {
    if (isUnsupportedHostMethod(error)) return fallback;
    throw error;
  });
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
        emptyOnUnsupported(link.getTeammateTopology(workspaceId), EMPTY_TEAM_TOPOLOGY),
        emptyOnUnsupported(link.listHitlPending(), []),
        link.getOrchestrationHealth().catch(() => ({ suspendedAgents: 0, pendingHitl: 0 })),
      ]).then(([topology, pending, health]) => ({ topology, pending, health })),
      [signal, context?.signal],
    ),
    REMOTE_ROSTER_STALE_TIME_MS,
  );
}

/** Query-backed Agent history; callers may apply a local capability breaker. */
export function fetchRemoteAgentHistory(
  link: RemoteLink,
  queryClient: RemoteQueryClientLike | undefined,
  sessionId: number,
  limit = 24,
  signal?: AbortSignal,
  offset = 0,
  query = '',
): Promise<AgentHistoryReply[]> {
  return fetchRemoteQuery(
    queryClient,
    remoteQueryKeys.agentHistory(sessionId, limit, offset, query),
    (context) => abortable(
      offset === 0 && !query
        ? link.listAgentHistory(limit)
        : link.listAgentHistory(limit, offset, query),
      [signal, context?.signal],
    ),
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
function requestWithAbortTimeout<T>(
  signal: AbortSignal | undefined,
  timeoutMs: number,
  timeoutMessage: string,
  start: (resolve: (value: T) => void, reject: (reason: unknown) => void) => (() => void),
): Promise<T> {
  return new Promise((resolve, reject) => {
    let cleanupRequest = () => {};
    let timer: ReturnType<typeof setTimeout> | undefined;
    let settled = false;
    const cleanup = () => {
      cleanupRequest();
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
    try {
      timer = setTimeout(() => finish(() => reject(new Error(`${timeoutMessage} timed out after ${timeoutMs}ms`))), timeoutMs);
      cleanupRequest = start(
        (value) => finish(() => resolve(value)),
        (error) => finish(() => reject(error)),
      );
    } catch (error) {
      finish(() => reject(error));
    }
  });
}

export function requestPaneSnapshot(
  link: RemoteLink,
  workspaceId: string,
  signal?: AbortSignal,
  timeoutMs = REMOTE_QUERY_TIMEOUT_MS,
): Promise<PaneInfo[]> {
  return requestWithAbortTimeout(signal, timeoutMs, 'list panes', (resolve) => {
    const stop = link.onMessage((message: WsMessage) => {
      if (message.type !== 'panes' || message.workspaceId !== workspaceId) return;
      resolve(dedupeRemoteItems(message.panes));
    });
    link.listPanes();
    return stop;
  });
}

export function requestWorkspaceSnapshot(
  link: RemoteLink,
  signal?: AbortSignal,
  timeoutMs = REMOTE_QUERY_TIMEOUT_MS,
): Promise<WorkspaceInfo[]> {
  return requestWithAbortTimeout(signal, timeoutMs, 'list workspaces', (resolve, reject) => {
    link.listWorkspaces().then(
      (result) => resolve(dedupeRemoteItems(result.workspaces ?? [])),
      reject,
    );
    return () => {};
  });
}

export async function confirmedWorkspaceTarget(
  switchWorkspace: (workspaceId: string) => Promise<boolean>,
  workspaceId: string,
  paneId: string | null,
): Promise<{ workspaceId: string; paneId: string | null } | null> {
  return await switchWorkspace(workspaceId) ? { workspaceId, paneId } : null;
}
