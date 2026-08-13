import type {
  PaneInfo,
  PaneRef,
  RemoteLink,
  WorkspaceInfo,
} from '@ridge/remote';
import { unknownText } from '@ridge/remote/shared/transport/unknownText';

export interface HostForestPane {
  id: string;
  title: string;
  cwd?: string;
  isAgent: boolean;
}

export interface HostForestWorkspace {
  id: string;
  name: string;
  active: boolean;
  panes: HostForestPane[];
}

export interface HostForestSource {
  hostId: string;
  link: HostForestLink;
  signal?: AbortSignal;
}

export type HostForestLink = Pick<
  RemoteLink,
  'listWorkspaces' | 'listWorkspacePanes'
>;

export interface HostTopologyLink extends HostForestLink {
  state(): string;
  disconnect(): void;
  switchWorkspace(workspaceId: string): Promise<boolean>;
  createWorkspace(name?: string): Promise<string | null>;
  renameWorkspace?(workspaceId: string, name: string): Promise<boolean>;
  saveWorkspace?(workspaceId: string, name: string): Promise<boolean>;
  createPane(shell?: string): Promise<string | null>;
  closePane(pane: PaneRef): Promise<boolean>;
  closeWorkspace(workspaceId: string): Promise<boolean>;
  onRawBytes(fn: (pane: PaneRef, bytes: Uint8Array) => void): () => void;
  subscribePane(pane: PaneRef): void;
  /** Promote a subscribed pane to the host's latency-priority stream. */
  promotePane?(pane: PaneRef): void;
  sendStdin(pane: PaneRef, data: string): void;
  enqueueStdinTask?: NonNullable<RemoteLink['enqueueStdinTask']>;
  refreshPane(
    pane: PaneRef,
    rows: number,
    cols: number,
    pixelWidth: number,
    pixelHeight: number,
  ): void;
  getPaneOutput(pane: PaneRef): string[];
  markPaneAgent?(
    workspaceId: string,
    paneId: string,
    on: boolean,
    agentId?: string,
  ): Promise<void>;
  listShells?: NonNullable<RemoteLink['listShells']>;
  changePaneShell?: NonNullable<RemoteLink['changePaneShell']>;
  /** Read-only filesystem probes must execute on this host, never on the
   * controller's local filesystem. */
  inspectPath?(path: string, signal?: AbortSignal): Promise<{ exists: boolean; isDirectory?: boolean }>;
}

export interface HostForestResult {
  hostId: string;
  workspaces: HostForestWorkspace[];
  error?: string;
  warning?: string;
  loading?: boolean;
  loadedWorkspaces?: number;
  totalWorkspaces?: number;
  loadedWorkspaceIds?: string[];
  failedWorkspaceIds?: string[];
}

export type HostForestProgressListener = (result: HostForestResult) => void;

export interface HostTopologyRefreshJob {
  hostId: string;
  refresh(): Promise<HostForestResult | null>;
}

function abortable<T>(promise: Promise<T>, signal?: AbortSignal): Promise<T> {
  if (!signal) return promise;
  if (signal.aborted) return Promise.reject(new Error('请求已取消'));
  return new Promise<T>((resolve, reject) => {
    const onAbort = () => reject(new Error('请求已取消'));
    signal.addEventListener('abort', onAbort, { once: true });
    promise.then(
      (value) => {
        signal.removeEventListener('abort', onAbort);
        resolve(value);
      },
      (error) => {
        signal.removeEventListener('abort', onAbort);
        reject(error);
      },
    );
  });
}

export const HOST_FOREST_REQUEST_TIMEOUT_MS = 15_000;

/** Timeout wrapper for a single host topology request. */
function abortableWithTimeout<T>(
  promise: Promise<T>,
  signal?: AbortSignal,
  timeoutMs = HOST_FOREST_REQUEST_TIMEOUT_MS,
): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    let settled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const cleanup = () => {
      if (timer !== undefined) clearTimeout(timer);
      signal?.removeEventListener('abort', onAbort);
    };
    const finishResolve = (value: T) => {
      if (settled) return;
      settled = true;
      cleanup();
      resolve(value);
    };
    const finishReject = (error: unknown) => {
      if (settled) return;
      settled = true;
      cleanup();
      reject(error instanceof Error ? error : new Error(unknownText(error)));
    };
    const onAbort = () => finishReject(new Error('request aborted'));
    if (signal?.aborted) {
      onAbort();
      return;
    }
    signal?.addEventListener('abort', onAbort, { once: true });
    if (timeoutMs > 0) {
      timer = setTimeout(
        () => finishReject(new Error(`remote topology timeout (${timeoutMs}ms)`)),
        timeoutMs,
      );
    }
    promise.then(finishResolve, finishReject);
  });
}

/** Failure keeps the last successful tree visible; success replaces it. */
export function retainHostForest(
  previous: HostForestResult | undefined,
  next: HostForestResult,
): HostForestResult {
  if (next.error && previous) return { ...next, workspaces: previous.workspaces };
  if (!previous || (!next.loading && !next.failedWorkspaceIds?.length)) return next;
  const loaded = new Set(next.loadedWorkspaceIds ?? []);
  const failed = new Set(next.failedWorkspaceIds ?? []);
  const previousById = new Map(previous.workspaces.map((workspace) => [workspace.id, workspace]));
  return {
    ...next,
    workspaces: next.workspaces.map((workspace) =>
      loaded.has(workspace.id) && !failed.has(workspace.id)
        ? workspace
        : { ...workspace, panes: previousById.get(workspace.id)?.panes ?? workspace.panes }),
  };
}

export function hostTopologyErrorKind(error?: string): 'auth' | 'retryable' {
  return error && /(?:auth|unauthor|forbidden|totp|登录|鉴权|认证|401|403)/i.test(error)
    ? 'auth'
    : 'retryable';
}

function paneNode(pane: PaneInfo): HostForestPane {
  return {
    id: pane.id,
    title: pane.title?.trim() || pane.id,
    cwd: pane.cwd,
    isAgent: pane.isAgent === true,
  };
}

async function workspaceNode(
  link: HostForestLink,
  workspace: WorkspaceInfo,
  signal?: AbortSignal,
): Promise<HostForestWorkspace> {
  const panes = await abortableWithTimeout(link.listWorkspacePanes(workspace.id), signal);
  return {
    id: workspace.id,
    name: workspace.name?.trim() || `工作区 ${workspace.id.slice(0, 8)}`,
    active: workspace.active,
    panes: panes.map(paneNode),
  };
}

/** 每 host 独立失败；同 host 各 workspace pane 列表并行，结果仍按 host 返回。 */
export async function loadHostForest(
  sources: readonly HostForestSource[],
  onProgress?: HostForestProgressListener,
): Promise<HostForestResult[]> {
  return Promise.all(
    sources.map(async ({ hostId, link, signal }) => {
      try {
        const { workspaces } = await abortable(link.listWorkspaces(), signal);
        const nodes: HostForestWorkspace[] = workspaces.map((workspace) => ({
          id: workspace.id,
          name: workspace.name?.trim() || `工作区 ${workspace.id.slice(0, 8)}`,
          active: workspace.active,
          panes: [],
        }));
        onProgress?.({
          hostId,
          workspaces: [...nodes],
          loading: workspaces.length > 0,
          loadedWorkspaces: 0,
          totalWorkspaces: workspaces.length,
          loadedWorkspaceIds: [],
        });

        const failures: string[] = [];
        const failedWorkspaceIds: string[] = [];
        let loadedWorkspaces = 0;
        const loadedWorkspaceIds: string[] = [];
        await Promise.all(workspaces.map(async (workspace, index) => {
          let loaded = false;
          try {
            nodes[index] = await workspaceNode(link, workspace, signal);
            loaded = true;
          } catch (error) {
            failedWorkspaceIds.push(workspace.id);
            failures.push(
              `${nodes[index].name}: ${error instanceof Error ? error.message : unknownText(error)}`,
            );
          } finally {
            loadedWorkspaces += 1;
            if (loaded) loadedWorkspaceIds.push(workspace.id);
            onProgress?.({
              hostId,
              workspaces: [...nodes],
              loading: loadedWorkspaces < workspaces.length,
              loadedWorkspaces,
              totalWorkspaces: workspaces.length,
              loadedWorkspaceIds: [...loadedWorkspaceIds],
              failedWorkspaceIds: [...failedWorkspaceIds],
              warning: failures.length > 0 ? failures.join('; ') : undefined,
            });
          }
        }));
        return failures.length > 0
          ? {
              hostId,
              workspaces: nodes,
              warning: failures.join('; '),
              loadedWorkspaceIds,
              failedWorkspaceIds,
            }
          : { hostId, workspaces: nodes };
      } catch (error) {
        return {
          hostId,
          workspaces: [],
          error: error instanceof Error ? error.message : unknownText(error),
        };
      }
    }),
  );
}

/** Run hosts concurrently but publish each result as soon as that host settles.
 * A slow transport therefore cannot hide already-ready sibling forests. */
export async function settleHostTopologyRefreshes(
  jobs: readonly HostTopologyRefreshJob[],
  onSettled: (result: HostForestResult) => void,
): Promise<HostForestResult[]> {
  const settled = await Promise.all(
    jobs.map(async ({ hostId, refresh }) => {
      let result: HostForestResult | null;
      try {
        result = await refresh();
      } catch (error) {
        result = {
          hostId,
          workspaces: [],
          error: error instanceof Error ? error.message : unknownText(error),
        };
      }
      if (result) onSettled(result);
      return result;
    }),
  );
  return settled.filter((result): result is HostForestResult => result !== null);
}
