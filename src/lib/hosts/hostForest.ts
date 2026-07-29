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
): Promise<HostForestWorkspace> {
  const panes = await link.listWorkspacePanes(workspace.id);
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
    sources.map(async ({ hostId, link }) => {
      try {
        const { workspaces } = await link.listWorkspaces();
        return {
          hostId,
          workspaces: await Promise.all(
            workspaces.map((workspace) => workspaceNode(link, workspace)),
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
