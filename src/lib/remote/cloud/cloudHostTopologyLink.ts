import {
  createCloudWebrtcTransportWith,
  PaneRpcScheduler,
  RpcClient,
  paneRefKey,
  type CloudConnectionCallbacks,
  type CloudWebrtcAdapter,
  type PaneInfo,
  type PaneRef,
  type RemoteShellInfo,
  type WorkspaceInfo,
} from '@ridge/remote';
import { ControllerCloudProvider } from '@ridge/remote/shared/cloud/controllerCloudProvider';
import { snapshot as authSnapshot } from '@ridge/remote/shared/cloud/auth';
import type { HostTopologyLink } from '$lib/hosts/hostForest';
import type { PaneNode } from '$lib/types';
import { verifyTotpOverControl } from './cloudControllerBoot';

interface BackendWorkspace {
  id: string;
  name?: string;
}

function flattenPanes(node: PaneNode | null | undefined): PaneInfo[] {
  if (!node) return [];
  if (node.type === 'leaf') {
    return node.id
      ? [{
          id: node.id,
          title: node.title,
          cwd: node.cwd,
          isAgent: node.agent_state === 'busy',
        }]
      : [];
  }
  return node.children.flatMap(flattenPanes);
}

export class CloudHostTopologyLink implements HostTopologyLink {
  private readonly workspaceByPane = new Map<string, string>();
  private readonly livePanes = new Set<string>();
  private readonly scheduler: PaneRpcScheduler;
  private readonly stopReconnectResume: () => void;

  constructor(
    private readonly adapter: CloudWebrtcAdapter,
    private readonly rpc: RpcClient,
  ) {
    this.scheduler = new PaneRpcScheduler(rpc);
    this.stopReconnectResume = rpc.onReconnected(() => this.scheduler.resumeAll());
  }

  state() {
    return this.adapter.state();
  }

  disconnect(): void {
    this.stopReconnectResume();
    this.scheduler.dispose();
    this.rpc.dispose();
    this.adapter.close();
    this.adapter.dispose();
  }

  async listWorkspaces(): Promise<{ workspaces: WorkspaceInfo[] }> {
    const [rows, activeId] = await Promise.all([
      this.rpc.request<BackendWorkspace[]>('list_workspaces'),
      this.rpc.request<string>('get_active_workspace_id'),
    ]);
    return {
      workspaces: rows.map((workspace) => ({
        id: workspace.id,
        name: workspace.name,
        active: workspace.id === activeId,
      })),
    };
  }

  async listWorkspacePanes(workspaceId: string): Promise<PaneInfo[]> {
    const tree = await this.rpc.request<PaneNode>('get_pane_layout_for', { workspaceId });
    const panes = flattenPanes(tree);
    const liveIds = new Set(panes.map((pane) => pane.id));
    for (const [paneId, ownerWorkspaceId] of this.workspaceByPane) {
      if (ownerWorkspaceId === workspaceId && !liveIds.has(paneId)) {
        this.retirePane({ workspaceId, paneId });
        this.workspaceByPane.delete(paneId);
      }
    }
    for (const pane of panes) {
      this.activatePane({ workspaceId, paneId: pane.id });
    }
    return panes;
  }

  async closePane(pane: PaneRef): Promise<boolean> {
    this.retirePane(pane);
    try {
      await this.rpc.request('close_pane', {
        workspaceId: pane.workspaceId,
        paneId: pane.paneId,
      });
      if (this.workspaceByPane.get(pane.paneId) === pane.workspaceId) {
        this.workspaceByPane.delete(pane.paneId);
      }
      return true;
    } catch {
      this.activatePane(pane);
      return false;
    }
  }

  async switchWorkspace(workspaceId: string): Promise<boolean> {
    try {
      await this.rpc.request('switch_workspace', { workspaceId });
      return true;
    } catch {
      return false;
    }
  }

  async createWorkspace(name?: string): Promise<string | null> {
    try {
      return await this.rpc.request<string>('create_workspace', name ? { name } : {});
    } catch {
      return null;
    }
  }

  async renameWorkspace(workspaceId: string, name: string): Promise<boolean> {
    try {
      await this.rpc.request('rename_workspace', { workspaceId, name });
      return true;
    } catch {
      return false;
    }
  }

  async saveWorkspace(workspaceId: string, name: string): Promise<boolean> {
    try {
      await this.rpc.request('save_workspace_to_file', { workspaceId, name });
      return true;
    } catch {
      return false;
    }
  }

  async createPane(): Promise<string | null> {
    try {
      const tree = await this.rpc.request<PaneNode>('get_pane_layout');
      const first = flattenPanes(tree)[0];
      if (!first) return null;
      const result = await this.rpc.request<{ pane_id: string }>('split_pane', {
        paneId: first.id,
        direction: 'horizontal',
      });
      return result.pane_id || null;
    } catch {
      return null;
    }
  }

  async closeWorkspace(workspaceId: string): Promise<boolean> {
    const panes = [...this.workspaceByPane]
      .filter(([, ownerWorkspaceId]) => ownerWorkspaceId === workspaceId)
      .map(([paneId]) => ({ workspaceId, paneId }));
    for (const pane of panes) this.retirePane(pane);
    try {
      await this.rpc.request('close_workspace', { workspaceId });
      for (const pane of panes) this.workspaceByPane.delete(pane.paneId);
      return true;
    } catch {
      for (const pane of panes) this.activatePane(pane);
      return false;
    }
  }

  async markPaneAgent(
    workspaceId: string,
    paneId: string,
    on: boolean,
    agentId?: string,
  ): Promise<void> {
    const pane = { workspaceId, paneId };
    if (!this.livePanes.has(paneRefKey(pane))) throw new Error(`Pane not active: ${paneId}`);
    await this.rpc.request(
      on ? 'register_teammate_agent' : 'release_teammate_agent',
      on
        ? { workspaceId, paneId, agentId: agentId || 'agent' }
        : { workspaceId, paneId },
      { scope: paneRefKey(pane) },
    );
  }

  async listShells(): Promise<RemoteShellInfo[]> {
    return this.rpc.request<RemoteShellInfo[]>('detect_available_shells');
  }

  async changePaneShell(
    workspaceId: string,
    paneId: string,
    shell: RemoteShellInfo,
  ): Promise<void> {
    const pane = { workspaceId, paneId };
    const scope = paneRefKey(pane);
    if (!this.livePanes.has(scope)) throw new Error(`Pane not active: ${paneId}`);
    await this.rpc.request('change_pane_shell', {
      paneId,
      shell: shell.program,
      args: shell.args ?? [],
    }, { scope });
    await this.rpc.request('activate_pane_pty', { workspaceId, paneId }, { scope });
  }

  onRawBytes(fn: (pane: PaneRef, bytes: Uint8Array) => void): () => void {
    return this.adapter.onPaneBytes((paneId, bytes) => {
      const workspaceId = this.workspaceByPane.get(paneId);
      if (workspaceId) fn({ workspaceId, paneId }, bytes);
    });
  }

  subscribePane(pane: PaneRef): void {
    this.activatePane(pane);
    this.rpc.notify('subscribe-pane', {
      workspaceId: pane.workspaceId,
      paneId: pane.paneId,
    });
  }

  sendStdin(pane: PaneRef, data: string): boolean {
    if (!this.livePanes.has(paneRefKey(pane))) return false;
    return this.scheduler.enqueueInput(pane, data);
  }

  refreshPane(
    pane: PaneRef,
    rows: number,
    cols: number,
    _pixelWidth: number,
    _pixelHeight: number,
  ): void {
    if (!this.livePanes.has(paneRefKey(pane))) return;
    this.scheduler.scheduleResize(pane, rows, cols);
  }

  getPaneOutput(_pane: PaneRef): string[] {
    return [];
  }

  get rpcSchedulingDiagnostics() {
    return this.scheduler.diagnostics;
  }

  private activatePane(pane: PaneRef): void {
    this.workspaceByPane.set(pane.paneId, pane.workspaceId);
    this.livePanes.add(paneRefKey(pane));
    this.scheduler.resume(pane);
  }

  private retirePane(pane: PaneRef): void {
    const key = paneRefKey(pane);
    this.livePanes.delete(key);
    this.scheduler.retire(pane);
  }
}

export async function connectCloudHostTopologyLink(
  hostDevice: string,
  totp: string,
): Promise<HostTopologyLink> {
  const auth = authSnapshot();
  const username = auth.user?.username;
  if (!auth.userToken || !username) throw new Error('公网接入需登录含用户名的 Ridge 账户');

  let provider!: ControllerCloudProvider;
  let resolveConnected!: () => void;
  let rejectConnected!: (error: Error) => void;
  const connected = new Promise<void>((resolve, reject) => {
    resolveConnected = resolve;
    rejectConnected = reject;
  });
  const adapter = createCloudWebrtcTransportWith(hostDevice, (adapterCallbacks) => {
    const callbacks: CloudConnectionCallbacks = {
      onState: (state) => {
        adapterCallbacks.onState?.(state);
        if (state === 'connected') resolveConnected();
        if (state === 'error') rejectConnected(new Error('公网主机连接失败'));
      },
      onFrame: (frame) => adapterCallbacks.onFrame?.(frame),
      onError: (message) => rejectConnected(new Error(message)),
    };
    provider = new ControllerCloudProvider({
      userToken: () => authSnapshot().userToken || auth.userToken!,
      username,
      baseDomain: undefined,
    }, callbacks);
    return provider;
  });

  const rpc = new RpcClient(adapter);
  const timer = setTimeout(() => rejectConnected(new Error('公网主机连接超时')), 20_000);
  try {
    void adapter.connect();
    await connected;
    const verified = await verifyTotpOverControl(adapter, totp);
    if (!verified) throw new Error('TOTP 验证失败');
    rpc.hello();
    return new CloudHostTopologyLink(adapter, rpc);
  } catch (error) {
    rpc.dispose();
    adapter.close();
    adapter.dispose();
    throw error;
  } finally {
    clearTimeout(timer);
  }
}
