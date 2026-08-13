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
import {
  tryEnqueuePaneInput,
  tryEnqueuePaneInputImmediate,
  retirePaneInput,
} from '@ridge/remote/shared/terminal/paneInputGate';

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
          // `idle` and `starting` are still Agent panes; `busy` is only the
          // runtime state, not the identity marker. Keep the marker true when
          // a host sends either field so Agent cards and Pane state stay aligned.
          isAgent: node.agent_state !== undefined || node.agent_id !== undefined,
          ...(node.agent_state ? { agentState: node.agent_state } : {}),
          ...(node.agent_id ? { agentId: node.agent_id } : {}),
        }]
      : [];
  }
  return node.children.flatMap(flattenPanes);
}

export class CloudHostTopologyLink implements HostTopologyLink {
  /** Composite pane identity registry. Raw frames remain legacy paneId-only. */
  private readonly workspaceByPane = new Map<string, PaneRef>();
  private readonly livePanes = new Set<string>();
  private activeWorkspaceId: string | null = null;
  /** Panes actually attached to a foreign Host. `livePanes` also contains
   * panes discovered while projecting a workspace, so it is not a safe
   * subscription list to replay after the Host bridge is recreated. */
  private readonly subscribedPanes = new Map<string, PaneRef>();
  private activePaneKey: string | null = null;
  private readonly scheduler: PaneRpcScheduler;
  private readonly stopReconnectResume: () => void;

  constructor(
    private readonly adapter: CloudWebrtcAdapter,
    private readonly rpc: RpcClient,
  ) {
    this.scheduler = new PaneRpcScheduler(rpc);
    this.stopReconnectResume = rpc.onReconnected(() => {
      // A reconnect creates a fresh Host bridge: its pane subscriptions and
      // active-pane QoS state are gone. Replay only panes that were actually
      // attached (not every pane discovered by listWorkspacePanes), preserving
      // the focused pane as the latency-critical stream.
      const activeKey = this.activePaneKey;
      this.activePaneKey = null;
      for (const [key, pane] of this.subscribedPanes) {
        const active = key === activeKey;
        this.rpc.notify('subscribe-pane', {
          workspaceId: pane.workspaceId,
          paneId: pane.paneId,
          active,
        });
        if (active) this.activePaneKey = key;
      }
      this.scheduler.resumeAll();
    });
  }

  state() {
    return this.adapter.state();
  }

  disconnect(): void {
    for (const key of this.livePanes) retirePaneInput(key);
    this.livePanes.clear();
    this.subscribedPanes.clear();
    this.workspaceByPane.clear();
    this.activePaneKey = null;
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
    this.activeWorkspaceId = activeId || null;
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
    for (const [key, pane] of this.workspaceByPane) {
      if (pane.workspaceId === workspaceId && !liveIds.has(pane.paneId)) {
        this.retirePane(pane);
        this.workspaceByPane.delete(key);
      }
    }
    for (const pane of panes) {
      this.activatePane({ workspaceId, paneId: pane.id });
    }
    return panes;
  }

  async closePane(pane: PaneRef): Promise<boolean> {
    const key = paneRefKey(pane);
    const subscribed = this.subscribedPanes.get(key);
    const wasActive = this.activePaneKey === key;
    this.retirePane(pane);
    try {
      await this.rpc.request('close_pane', {
        workspaceId: pane.workspaceId,
        paneId: pane.paneId,
      });
      return true;
    } catch {
      this.activatePane(pane);
      if (subscribed) this.subscribedPanes.set(key, subscribed);
      if (wasActive) this.activePaneKey = key;
      return false;
    }
  }

  async switchWorkspace(workspaceId: string): Promise<boolean> {
    try {
      await this.rpc.request('switch_workspace', { workspaceId });
      this.activeWorkspaceId = workspaceId;
      return true;
    } catch {
      return false;
    }
  }

  async createWorkspace(name?: string): Promise<string | null> {
    // Let the Host operation surface transport/auth failures. Returning null
    // here made a failed create indistinguishable from a valid empty result.
    const workspaceId = await this.rpc.request<string>('create_workspace', name ? { name } : {});
    this.activeWorkspaceId = workspaceId || this.activeWorkspaceId;
    return workspaceId;
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
      const workspaceId = this.activeWorkspaceId;
      if (!workspaceId) return null;
      const tree = await this.rpc.request<PaneNode>('get_pane_layout_for', { workspaceId });
      const first = flattenPanes(tree)[0];
      if (!first) return null;
      const result = await this.rpc.request<{ pane_id: string }>('split_pane', {
        workspaceId,
        paneId: first.id,
        direction: 'horizontal',
      });
      return result.pane_id || null;
    } catch {
      return null;
    }
  }

  async closeWorkspace(workspaceId: string): Promise<boolean> {
    const panes = [...this.workspaceByPane.values()]
      .filter((pane) => pane.workspaceId === workspaceId);
    const subscriptions = new Map(
      panes
        .map((pane) => [paneRefKey(pane), this.subscribedPanes.get(paneRefKey(pane))] as const)
        .filter((entry): entry is [string, PaneRef] => entry[1] !== undefined),
    );
    const activeKey = this.activePaneKey;
    for (const pane of panes) this.retirePane(pane);
    try {
      await this.rpc.request('close_workspace', { workspaceId });
      return true;
    } catch {
      for (const pane of panes) this.activatePane(pane);
      for (const [key, pane] of subscriptions) this.subscribedPanes.set(key, pane);
      if (activeKey && subscriptions.has(activeKey)) this.activePaneKey = activeKey;
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
      workspaceId,
      paneId,
      shell: shell.program,
      args: shell.args ?? [],
    }, { scope });
    await this.rpc.request('activate_pane_pty', { workspaceId, paneId }, { scope });
  }

  onRawBytes(fn: (pane: PaneRef, bytes: Uint8Array) => void): () => void {
    return this.adapter.onPaneBytes((paneId, bytes) => {
      const matches = [...this.workspaceByPane.values()]
        .filter((pane) => pane.paneId === paneId);
      // Legacy cloud raw frames carry no workspaceId. Never guess when the
      // same pane id is live in more than one workspace.
      if (matches.length === 1) fn(matches[0], bytes);
    });
  }

  async inspectPath(path: string, signal?: AbortSignal): Promise<{ exists: boolean; isDirectory?: boolean }> {
    if (signal?.aborted) return { exists: false };
    try {
      const node = await this.rpc.request<{ is_dir?: boolean }>(
        'get_file_tree',
        { path, depth: 0 },
        { signal },
      );
      return { exists: true, isDirectory: node.is_dir === true };
    } catch (error) {
      if (signal?.aborted) return { exists: false };
      const detail = error instanceof Error ? error.message : String(error);
      if (/not a directory/i.test(detail)) return { exists: true, isDirectory: false };
      if (/not exist|no such file|not found/i.test(detail)) return { exists: false };
      throw error;
    }
  }

  subscribePane(pane: PaneRef): void {
    this.activatePane(pane);
    this.subscribedPanes.set(paneRefKey(pane), pane);
    this.rpc.notify('subscribe-pane', {
      workspaceId: pane.workspaceId,
      paneId: pane.paneId,
      // A newly-bound foreign pane must receive its initial live tail while
      // the local component attaches. Focus changes are promoted below.
      active: true,
    });
    this.activePaneKey = paneRefKey(pane);
  }

  promotePane(pane: PaneRef): void {
    const key = paneRefKey(pane);
    if (!this.livePanes.has(key) || this.activePaneKey === key) return;
    this.activePaneKey = key;
    // Host treats a duplicate subscribe as an idempotent QoS promotion; no
    // second PTY fan-out is created for an already-registered pane.
    this.rpc.notify('subscribe-pane', {
      workspaceId: pane.workspaceId,
      paneId: pane.paneId,
      active: true,
    });
  }

  sendStdin(pane: PaneRef, data: string): boolean {
    if (!this.livePanes.has(paneRefKey(pane))) return false;
    const key = paneRefKey(pane);
    return tryEnqueuePaneInputImmediate(key, () => {
      this.scheduler.enqueueInput(pane, data);
    });
  }

  enqueueStdinTask(pane: PaneRef, task: () => Promise<string | null> | string | null): boolean {
    const key = paneRefKey(pane);
    if (!this.livePanes.has(key)) return false;
    return tryEnqueuePaneInput(key, async () => {
      const data = await task();
      if (data) this.scheduler.enqueueInput(pane, data);
    });
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
    this.workspaceByPane.set(paneRefKey(pane), pane);
    this.livePanes.add(paneRefKey(pane));
    this.scheduler.resume(pane);
  }

  private retirePane(pane: PaneRef): void {
    const key = paneRefKey(pane);
    this.workspaceByPane.delete(key);
    this.subscribedPanes.delete(key);
    this.livePanes.delete(key);
    if (this.activePaneKey === key) this.activePaneKey = null;
    this.scheduler.retire(pane);
    retirePaneInput(key);
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
    adapter.connect().catch((error) => {
      rejectConnected(error instanceof Error ? error : new Error(String(error)));
    });
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
