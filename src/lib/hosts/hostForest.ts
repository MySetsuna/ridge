import type {
  PaneInfo,
  PaneRef,
  RemoteLink,
  WorkspaceInfo,
} from '@ridge/remote';

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
  sendStdin(pane: PaneRef, data: string): void;
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
  listShells?: RemoteLink['listShells'];
  changePaneShell?: RemoteLink['changePaneShell'];
}

export interface HostForestResult {
  hostId: string;
  workspaces: HostForestWorkspace[];
  error?: string;
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

/** Failure keeps the last successful tree visible; success replaces it. */
export function retainHostForest(
  previous: HostForestResult | undefined,
  next: HostForestResult,
): HostForestResult {
  return next.error && previous
    ? { ...next, workspaces: previous.workspaces }
    : next;
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
  const panes = await abortable(link.listWorkspacePanes(workspace.id), signal);
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
): Promise<HostForestResult[]> {
  return Promise.all(
    sources.map(async ({ hostId, link, signal }) => {
      try {
        const { workspaces } = await abortable(link.listWorkspaces(), signal);
        return {
          hostId,
          workspaces: await Promise.all(
            workspaces.map((workspace) => workspaceNode(link, workspace, signal)),
          ),
        };
      } catch (error) {
        return {
          hostId,
          workspaces: [],
          error: error instanceof Error ? error.message : String(error),
        };
      }
    }),
  );
}
