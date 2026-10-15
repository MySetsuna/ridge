import type { TerminalInputContext, TerminalInputData } from '@ridge/remote/shared/transport/paneRpcScheduler';
import terminalHello from '@ridge/remote/shared/cloud/signaling/fixtures/terminal/hello.json';
// Remote UI metadata rides the scoped RPC bridge. Terminal snapshots/deltas
// arrive directly through the canonical adapter; each activation owns one stream.
// Auth/boot stays in CloudAuthScreen and the existing controller handle.

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { PaneNode } from '$lib/types';
import type { CloudControllerHandle } from '$lib/remote/cloud/cloudControllerBoot';
import { bridge, type TauriEvent } from '$lib/transport/tauriShim/bridge';
import {
  classifyFailure,
  PaneRpcScheduler,
  paneRefKey,
  RpcCancelledError,
  type RemoteLink,
  type PaneRef,
  type RpcRequestOptions,
  type ConnectionState,
  type ConnectionFailure,
  type PaneInfo,
  type WorkspaceInfo,
  type WsMessage,
  type RawByteListener,
  type TerminalFrameListener,
  type MetaListener,
  type PtyResizeListener,
  type PaneRenderOwner,
  type ThemeListener,
  type ThemeSnapshot,
  type TeammateTopology,
  type AgentMessageTarget,
  type AgentMessageReceipt,
  type AgentHistoryReply,
  type TeammateGroup,
  type TeammateGroupSnapshot,
  type HitlPendingItem,
  type HitlResolveOutcome,
  type RemoteShellInfo,
} from '@ridge/remote';
import {
  tryEnqueuePaneInput,
  tryEnqueuePaneInputImmediate,
  retirePaneInput,
} from '@ridge/remote/shared/terminal/paneInputGate';

/** Backend `list_workspaces` row (subset we use). */
interface BackendWorkspace {
  id: string;
  name?: string | null;
}

/** Host `get_active_theme_entry` / `get_theme_data` theme row (subset we use). */
interface ThemeEntryLite {
  id: string;
  type?: 'dark' | 'light';
  colors: Record<string, string>;
}

/** Default PTY grid for a freshly-activated pane until the canvas claims its real size. */
const DEFAULT_ROWS = 24;
const DEFAULT_COLS = 80;
// Input is latency-critical: admit the first byte immediately. The per-pane
// scheduler still keeps one request in flight and coalesces later bytes while
// that request is pending, so removing this artificial window does not create
// one RPC per keystroke under a burst.
const INPUT_BATCH_WINDOW_MS = 0;

// A transient listen/subscribe failure must not strand a Pane after the UI has
// recorded its subscription intent. Keep retries bounded and cancel them on
// reconnect/close so a dead transport cannot create a retry storm.
const SUBSCRIBE_RETRY_BASE_MS = 100;
const SUBSCRIBE_RETRY_MAX_MS = 2_000;
const SUBSCRIBE_MAX_RETRIES = 4;

interface SubscribeRetryIntent {
  pane: PaneRef;
  resume: boolean;
  active: boolean;
  attempts: number;
  activationId?: number;
}

/** Flatten a host pane-tree to the mobile's flat leaf list (server.rs's downgrade). */
function flattenLeaves(node: PaneNode | null | undefined): PaneInfo[] {
  if (!node) return [];
  if (node.type === 'leaf') {
    if (!node.id) return []; // pre-hydration placeholder leaf
    return [{
      id: node.id,
      title: node.title,
      cwd: node.cwd,
    }];
  }
  return node.children.flatMap(flattenLeaves);
}

interface CloudRemoteBridge {
  invoke<T>(
    cmd: string,
    args?: Record<string, unknown>,
    options?: RpcRequestOptions,
  ): Promise<T>;
  listen<T>(name: string, cb: (event: TauriEvent<T>) => void): Promise<UnlistenFn> | UnlistenFn;
  hasCapability(capability: string): boolean;
  onCapabilitiesChanged(fn: () => void): UnlistenFn;
}

const defaultCloudRemoteBridge: CloudRemoteBridge = {
  invoke<T>(cmd: string, args?: Record<string, unknown>, options?: RpcRequestOptions) {
    if (options) {
      return (invoke as unknown as CloudRemoteBridge['invoke'])<T>(cmd, args, options);
    }
    return args ? invoke<T>(cmd, args) : invoke<T>(cmd);
  },
  listen,
  hasCapability: (capability) => bridge.hasCapability(capability),
  onCapabilitiesChanged: (fn) => bridge.onCapabilitiesChanged(fn),
};

export class CloudRemoteConnection implements RemoteLink {
  private readonly handle: CloudControllerHandle;
  private readonly bridge: CloudRemoteBridge;
  private readonly fixedAuthorized: boolean;

  private _state: ConnectionState = 'connecting';
  // 最近一次失败分级（任务 A 问题1）。云端服务端「已认证但无权」会经信令 error 帧把
  // 稳定 code 透传到 provider onError → notifyError；据此区分用户问题/设备停用/通道异常。
  private _failure: ConnectionFailure | null = null;
  private _activeWorkspaceId = '';
  private _refreshSeq = 0;
  private readonly paneScheduler: PaneRpcScheduler;
  private readonly paneRpcControllers = new Map<string, AbortController>();
  private readonly closingPaneKeys = new Set<string>();
  private readonly deadPaneKeys = new Set<string>();
  private disposed = false;

  private readonly stateListeners = new Set<(s: ConnectionState) => void>();
  private readonly reconnectListeners = new Set<() => void>();
  private readonly messageListeners = new Set<(msg: WsMessage) => void>();
  private readonly rawByteListeners = new Set<RawByteListener>();
  private readonly terminalFrameListeners = new Set<TerminalFrameListener>();
  private terminalTransportUnlisten: UnlistenFn | null = null;
  private readonly terminalActivations = new Map<string, number>();
  private readonly metaListeners = new Set<MetaListener>();
  private readonly resizeListeners = new Set<PtyResizeListener>();
  private readonly themeListeners = new Set<ThemeListener>();

  // Notification admission is tracked independently of authoritative snapshots.
  private readonly paneSubscriptions = new Set<string>();
  /** Exact identities for the currently registered live streams. */
  private readonly streamPaneRefs = new Map<string, PaneRef>();
  // Panes whose subscribe is in flight (so concurrent subscribe calls stay idempotent).
  private readonly subscribing = new Set<string>();
  // Subscription intent survives one transient failure, but never beyond a
  // bounded retry window. The UI need not issue a second subscribe call.
  private readonly subscribeRetryIntents = new Map<string, SubscribeRetryIntent>();
  private readonly subscribeRetryTimers = new Map<string, ReturnType<typeof setTimeout>>();
  // Active QoS promotion is latest-wins and serialized. Rapid A→B→A switches
  // must not enqueue one Tauri Channel registration per tap.
  private activePaneKey: string | null = null;
  private activePromotionInFlight = false;
  private pendingActivePromotion: PaneRef | null = null;
  // pane-tree-changed unlisten (host-side layout changes → re-list panes).
  private treeUnlisten: UnlistenFn | null = null;
  private groupUnlisten: UnlistenFn | null = null;
  /** iter-60 G9：pane-meta-changed 退订句柄。 */
  private metaUnlisten: UnlistenFn | null = null;
  private resizeUnlisten: UnlistenFn | null = null;
  private terminalErrorUnlisten: UnlistenFn | null = null;
  private _lastTheme: ThemeSnapshot | null = null;
  // True once we've reached 'connected' at least once — gates reconnect handling
  // so the initial connect isn't treated as a reconnect.
  private _everConnected = false;
  // The §4 TOTP code the user verified, cached so a full re-handshake reconnect can
  // re-authorize without re-prompting (valid only while the code is in its time window).
  private _verifiedCode: string | null = null;
  // Guards against overlapping reconnect handling on a flapping link.
  private _reconnecting = false;

  constructor(
    handle: CloudControllerHandle,
    bridgeInstance: CloudRemoteBridge = defaultCloudRemoteBridge,
    options: { fixedAuthorized?: boolean } = {},
  ) {
    this.handle = handle;
    this.bridge = bridgeInstance;
    this.fixedAuthorized = options.fixedAuthorized === true;
    this.paneScheduler = new PaneRpcScheduler(
      {
        request: <T = unknown>(method: string, params?: unknown, rpcOptions?: RpcRequestOptions) => {
          const scope = rpcOptions?.scope;
          if (!scope) return Promise.reject(new RpcCancelledError(method));
          if (!params || typeof params !== 'object' || Array.isArray(params)) {
            return Promise.reject(new TypeError(`Invalid pane RPC params: ${method}`));
          }
          const signal = this._paneRpcSignalForKey(scope);
          if (!signal) return Promise.reject(new RpcCancelledError(method));
          return this.bridge.invoke<T>(method, params as Record<string, unknown>, {
            ...rpcOptions,
            signal,
          });
        },
        cancelScope: (scope) => this._abortPaneRpcs(scope),
      },
      { inputBatchWindowMs: INPUT_BATCH_WINDOW_MS },
    );
  }

  private _paneRpcSignalForKey(key: string): AbortSignal | null {
    if (
      this.disposed
      || this.closingPaneKeys.has(key)
      || this.deadPaneKeys.has(key)
    ) {
      return null;
    }
    let controller = this.paneRpcControllers.get(key);
    if (!controller || controller.signal.aborted) {
      controller = new AbortController();
      this.paneRpcControllers.set(key, controller);
    }
    return controller.signal;
  }

  private _paneRpcSignal(pane: PaneRef): AbortSignal | null {
    return this._paneRpcSignalForKey(paneRefKey(pane));
  }

  private _invokePane<T>(
    pane: PaneRef,
    cmd: string,
    args: Record<string, unknown>,
  ): Promise<T> {
    const signal = this._paneRpcSignal(pane);
    if (!signal) return Promise.reject(new RpcCancelledError(cmd));
    return this.bridge.invoke<T>(cmd, args, { signal });
  }

  private _abortPaneRpcs(key: string): number {
    const controller = this.paneRpcControllers.get(key);
    if (!controller) return 0;
    controller.abort();
    this.paneRpcControllers.delete(key);
    return 1;
  }

  private _abortAllPaneRpcs(): void {
    for (const controller of this.paneRpcControllers.values()) controller.abort();
    this.paneRpcControllers.clear();
  }

  private _deactivatePane(key: string, scopeAlreadyRetired = false): void {
    this.deadPaneKeys.add(key);
    this.closingPaneKeys.delete(key);
    retirePaneInput(key);
    if (!scopeAlreadyRetired) this.paneScheduler.retireScope(key);
    this.subscribing.delete(key);
    const retryTimer = this.subscribeRetryTimers.get(key);
    if (retryTimer) clearTimeout(retryTimer);
    this.subscribeRetryTimers.delete(key);
    this.subscribeRetryIntents.delete(key);
    if (this.activePaneKey === key) this.activePaneKey = null;
    if (this.pendingActivePromotion && paneRefKey(this.pendingActivePromotion) === key) {
      this.pendingActivePromotion = null;
    }
    this.paneSubscriptions.delete(key);
    this.streamPaneRefs.delete(key);
    this.terminalActivations.delete(key);
  }

  private _unsubscribePaneStream(pane: PaneRef): void {
    const key = paneRefKey(pane);
    this.subscribing.delete(key);
    const timer = this.subscribeRetryTimers.get(key);
    if (timer) clearTimeout(timer);
    this.subscribeRetryTimers.delete(key);
    this.subscribeRetryIntents.delete(key);
    this.paneSubscriptions.delete(key);
    this.streamPaneRefs.delete(key);
    this.terminalActivations.delete(key);
    if (this.activePaneKey === key) this.activePaneKey = null;
    if (this.pendingActivePromotion && paneRefKey(this.pendingActivePromotion) === key) {
      this.pendingActivePromotion = null;
    }
    this.handle.adapter.sendControl({
      jsonrpc: '2.0', method: 'unsubscribe-pane', params: pane,
    });
  }

  hasCapability(capability: string): boolean {
    return this.bridge.hasCapability(capability);
  }

  cacheScope(): string {
    return `cloud:${this.handle.hostDevice.trim().toLowerCase() || 'unknown-host'}`;
  }

  onCapabilitiesChanged(fn: () => void): () => void {
    return this.bridge.onCapabilitiesChanged(() => {
      if (!this.bridge.hasCapability('pane')) {
        this._failure = {
          category: 'channel',
          message: 'Remote 主机终端协议过旧，请升级 Ridge/rdg 后重连',
        };
        this.setState('error');
      }
      fn();
    });
  }

  /**
   * Bring the connection up: read the host's active workspace, watch host-side
   * layout changes, and flip to 'connected'. Must be awaited by App.svelte BEFORE
   * mounting MainApp, since MainApp.onMount calls listPanes()/refreshWorkspaces()
   * which depend on the active workspace id.
   */
  async init(): Promise<void> {
    this.terminalErrorUnlisten = await this.bridge.listen<{
      workspaceId: string; paneId: string; activationId: number;
    }>('pane-terminal-error', ({ payload }) => {
      if (!payload || typeof payload.workspaceId !== 'string' || typeof payload.paneId !== 'string' || !Number.isSafeInteger(payload.activationId)) return;
      const key = paneRefKey(payload);
      if (this.disposed || this.terminalActivations.get(key) !== payload.activationId) return;
      this._unsubscribePaneStream(payload);
      this.emitMessage({type:'event',name:'pane-terminal-error',payload});
    });
    if (this.disposed) { this.terminalErrorUnlisten(); this.terminalErrorUnlisten = null; return; }
    try {
      const stops: UnlistenFn[] = [];
      for (const name of ['commune-groups-changed','commune-task-changed','teammate://hitl-pending-changed']) {
        try {
          const stop = await this.bridge.listen(name, event => this.emitMessage({type:'event', name, payload:event.payload}));
          stops.push(stop);
        } catch { /* Explicit refresh remains available for unsupported events. */ }
      }
      this.groupUnlisten = () => { for (const stop of stops) stop(); };
      if (this.disposed) { this.groupUnlisten(); this.groupUnlisten = null; return; }
    } catch { /* Explicit refresh remains available when event subscription fails. */ }
    try {
      this._activeWorkspaceId = await this.bridge.invoke<string>('get_active_workspace_id');
    } catch {
      this._activeWorkspaceId = '';
    }
    // Host pushes `pane-tree-changed` when ITS layout mutates (desktop user splits /
    // closes / a teammate agent reshapes panes). Re-list so the mobile tracks it.
    try {
      this.treeUnlisten = await this.bridge.listen('pane-tree-changed', () => {
        void this._refreshPanes();
      });
    } catch {
      /* event subscribe failed — non-fatal, manual refresh still works */
    }
    // iter-60 G9: live pane meta (title/cwd) push — host aggregates OSC title/cwd
    // changes into `pane-meta-changed`, the cloud bridge forwards it as an event
    // frame. Feeds the same metaListeners the LAN leg's `pty-meta` uses, so the
    // header title/cwd and the pane-switcher popup refresh in real time instead
    // of waiting for the next layout poll.
    try {
      this.metaUnlisten = await this.bridge.listen<{ workspaceId?: string; paneId?: string; title?: string; cwd?: string }>(
        'pane-meta-changed',
        (e) => {
          const p = e.payload;
          if (!p || typeof p.paneId !== 'string' || typeof p.workspaceId !== 'string') return;
          const pane = {workspaceId:p.workspaceId,paneId:p.paneId};
          this.emitMessage({type:'event',name:'pane-meta-changed',payload:p});
          this.metaListeners.forEach((fn) => fn(pane, p.title ?? null, p.cwd ?? null));
        },
      );
    } catch {
      /* non-fatal — layout-poll fallback below still refreshes meta */
    }
    try {
      this.resizeUnlisten = await this.bridge.listen<{
        workspaceId?: string;
        paneId?: string;
        rows?: number;
        cols?: number;
        owner?: PaneRenderOwner;
      }>('pty-resized', (e) => {
        const payload = e.payload;
        if (!payload || typeof payload.paneId !== 'string') return;
        const paneId = payload.paneId;
        if (payload.workspaceId && payload.workspaceId !== this._activeWorkspaceId) return;
        if (typeof payload.rows !== 'number' || typeof payload.cols !== 'number') return;
        if (!Number.isFinite(payload.rows) || !Number.isFinite(payload.cols)) return;
        this.resizeListeners.forEach((fn) => fn(
          {
            workspaceId: payload.workspaceId ?? this._activeWorkspaceId,
            paneId,
          },
          payload.rows!,
          payload.cols!,
          payload.owner === 'remote' ? 'remote' : 'host',
        ));
      });
    } catch {
      /* non-fatal 鈥?manual refresh remains available when event forwarding is absent */
    }
    // Read the host's active theme so the mobile chrome + terminal match the desktop
    // (MainApp.onMount reads lastTheme() and applies it). Best-effort: an older host
    // without get_active_theme_entry just keeps the mobile's default palette.
    await this._loadTheme();
    this._everConnected = true;
    this.setState('connected');
  }

  /** Cache the verified §4 TOTP code for transparent re-auth on a full reconnect. */
  setVerifiedCode(code: string): void {
    this._verifiedCode = code;
  }

  /**
   * App.svelte/CloudAuthScreen forwards the provider's ongoing state here (drop /
   * reconnect / error). Maps it to the mobile {@link ConnectionState} so the UI shows
   * the live link status (断连提示), and drives auto-reconnect: on a connected-after-
   * down edge it re-authorizes (idempotent for an ICE-restart where the host stayed
   * verified; required after a full re-handshake where the host re-gated §4) and then
   * fires onReconnect so MainApp re-subscribes panes + re-lists workspaces.
   */
  notifyState(providerState: string): void {
    if (this.disposed) return;
    let mapped: ConnectionState = 'connecting';
    if (providerState === 'connected') mapped = 'connected';
    else if (providerState === 'error') mapped = 'error';
    else if (providerState === 'disconnected') mapped = 'disconnected';
    const wasDown = this._state !== 'connected';
    // 进入 'error' 但未带分级（provider 仅报状态、没有先经 notifyError 给出 code）→
    // 视为通道异常（网络/信令/WebRTC），UI 显示「通道异常」并允许重试，而非无限 pending。
    if (mapped === 'error' && !this._failure) {
      this._failure = { category: 'channel' };
    }
    this.setState(mapped);
    if (mapped === 'connected' && wasDown && this._everConnected) {
      void this._handleReconnect();
    }
    if (mapped === 'connected') this._everConnected = true;
  }

  /**
   * 服务端「已认证但无权」错误（任务 A 问题1）：provider 经信令 `{t:"error",code}` →
   * onError(message, code) 把稳定 code 透传到这里。按 code 分级（用户问题 / 设备停用 /
   * 通道）并进入 'error' 终态，使 UI 能精确处置（退回登录 / 去控制台 / 重试），不再
   * 无差别转圈。CloudAuthScreen 在 gate 通过后把 provider onError 转发到此方法。
   */
  notifyError(message: string, code?: string): void {
    if (this.disposed) return;
    const failure = classifyFailure(code);
    if (message) failure.message = message;
    this._failure = failure;
    this.setState('error');
  }

  /** Drop pane subscriptions and activation state. Shared by disconnect()
   *  and the reconnect path so a fresh transport re-subscribes from a clean slate. */
  private _teardownSubscriptions(): void {
    this._abortAllPaneRpcs();
    for (const timer of this.subscribeRetryTimers.values()) clearTimeout(timer);
    this.subscribeRetryTimers.clear();
    this.subscribeRetryIntents.clear();
    this.closingPaneKeys.clear();
    this.deadPaneKeys.clear();
    this.paneSubscriptions.clear();
    this.streamPaneRefs.clear();
    this.terminalActivations.clear();
    this.subscribing.clear();
    this.activePaneKey = null;
    this.activePromotionInFlight = false;
    this.pendingActivePromotion = null;
  }

  private async _handleReconnect(): Promise<void> {
    if (this.disposed || this._reconnecting) return;
    this._reconnecting = true;
    try {
      // Re-authorize. Prefer the signed 24h controller grant so a full reconnect
      // never depends on replaying an expired six-digit code. The in-memory code
      // remains only as a compatibility fallback for older hosts.
      let ok = this.fixedAuthorized;
      if (!this.fixedAuthorized) {
        ok = await this.handle.tryTrustGrant().catch(() => false);
        if (!ok && this._verifiedCode) {
          ok = await this.handle.verifyTotp(this._verifiedCode).catch(() => false);
        }
      }
      if (this.disposed) return;
      if (!ok) {
        // Trust expired/revoked and compatibility OTP also failed: keep the gate
        // closed and surface a recoverable channel error.
        this._failure = { category: 'channel' };
        this.setState('error');
        return;
      }
      // §reconnect-resub（修复「重连接上 scrollback 但没接上实时渲染」）：重连后的
      // 传输是一个**全新的 host 会话**，不持有任何 pane 订阅；但我们本地的
      // ptyUnlisten/subscribing 条目仍是断连前的旧值。subscribePane() 在
      // ptyUnlisten.has(paneId) 时会**提前返回**，导致 MainApp.onReconnect →
      // subscribePane 被吞掉：pane 既不会重挂 live `pty-output` 监听，也不会重新
      // register_pane_delta_channel → scrollback 从缓存重绘、但实时渲染不恢复。
      // 先清掉旧订阅状态，后续 subscribePane 才会在新传输上真正重跑 _subscribe。
      this._teardownSubscriptions();
      queueMicrotask(() => this.paneScheduler.resumeAll());
      this.reconnectListeners.forEach((fn) => {
        try { fn(); } catch { /* listener owns its errors */ }
      });
    } finally {
      this._reconnecting = false;
    }
  }

  private async _loadTheme(): Promise<void> {
    try {
      const entry = await this.bridge.invoke<ThemeEntryLite | null>('get_active_theme_entry');
      if (entry?.colors) {
        const themeType = entry.type === 'light' ? 'light' : 'dark';
        this._lastTheme = { id: entry.id, themeType, colors: entry.colors };
        this.themeListeners.forEach((fn) => fn(entry.colors, themeType));
      }
    } catch {
      /* older host without the command — keep the mobile default theme */
    }
  }

  // ── state / listeners ──────────────────────────────────────────────────────
  state(): ConnectionState {
    return this._state;
  }
  lastFailure(): ConnectionFailure | null {
    return this._failure;
  }
  private setState(s: ConnectionState): void {
    // 重新连上 / 开始新一轮连接 → 清掉旧失败分级，避免 UI 残留过期错误。
    if (s === 'connected' || s === 'connecting') this._failure = null;
    this._state = s;
    this.stateListeners.forEach((fn) => fn(s));
  }
  onStateChange(fn: (s: ConnectionState) => void): () => void {
    this.stateListeners.add(fn);
    return () => this.stateListeners.delete(fn);
  }
  onReconnect(fn: () => void): () => void {
    // Fired by _handleReconnect on a connected-after-down edge (after re-auth +
    // _teardownSubscriptions). MainApp's listener wipes the kernel, repaints from
    // cache, and re-subscribes the active pane so live rendering resumes on the
    // fresh transport.
    this.reconnectListeners.add(fn);
    return () => this.reconnectListeners.delete(fn);
  }
  onMessage(fn: (msg: WsMessage) => void): () => void {
    this.messageListeners.add(fn);
    return () => this.messageListeners.delete(fn);
  }
  onRawBytes(fn: RawByteListener): () => void {
    this.rawByteListeners.add(fn);
    return () => this.rawByteListeners.delete(fn);
  }
  onMetadata(fn: MetaListener): () => void {
    this.metaListeners.add(fn);
    return () => this.metaListeners.delete(fn);
  }
  onPtyResize(fn: PtyResizeListener): () => void {
    // Host and controller both publish canonical pty-resized events over the
    // bridge. The owner field identifies who initiated the refresh; receivers
    // only apply the announced grid and do not echo it.
    this.resizeListeners.add(fn);
    return () => this.resizeListeners.delete(fn);
  }
  onTheme(fn: ThemeListener): () => void {
    this.themeListeners.add(fn);
    return () => this.themeListeners.delete(fn);
  }
  lastTheme(): ThemeSnapshot | null {
    return this._lastTheme;
  }

  private emitMessage(msg: WsMessage): void {
    this.messageListeners.forEach((fn) => fn(msg));
  }

  // ── panes ──────────────────────────────────────────────────────────────────
  listPanes(): void {
    void this._refreshPanes();
  }

  private async _refreshPanes(): Promise<void> {
    const workspaceId = this._activeWorkspaceId;
    if (!workspaceId) return;
    let leaves: PaneInfo[];
    try {
      const layout = await this.bridge.invoke<PaneNode>('get_pane_layout_for', { workspaceId });
      leaves = flattenLeaves(layout);
    } catch {
      return; // host not ready / transient — leave the UI as-is
    }
    this.emitMessage({
      type: 'panes',
      workspaceId,
      panes: leaves,
    });
    // No native pty-meta event over cloud: derive title/cwd from the layout leaves
    // so the breadcrumb + sidebar cwd track (refreshed again on pane-tree-changed).
    for (const p of leaves) {
      const pane = { workspaceId, paneId: p.id };
      this.metaListeners.forEach((fn) => fn(pane, p.title ?? null, p.cwd ?? null));
    }
  }

  subscribePane(
    pane: PaneRef,
    opts?: { resume?: boolean; sinceSeq?: number; active?: boolean; activationId?: number },
  ): void {
    const { paneId, workspaceId } = pane;
    if (!paneId || !workspaceId) return;
    const key = paneRefKey(pane);
    if (this.disposed || this.closingPaneKeys.has(key) || this.deadPaneKeys.has(key)) return;
    const previousActivation = this.terminalActivations.get(key);
    const activation = opts?.activationId ?? previousActivation;
    if (!pane.workspaceId || !pane.paneId || !Number.isSafeInteger(activation) || activation! < 1) {
      throw new Error('terminal activation required');
    }
    this.terminalActivations.set(key, activation!);
    if (previousActivation !== activation) this.paneSubscriptions.delete(key);
    this.streamPaneRefs.set(key, pane);
    const previous = this.subscribeRetryIntents.get(key);
    this.subscribeRetryIntents.set(key, {
      pane,
      resume: opts?.resume ?? previous?.resume ?? false,
      active: opts?.active === true || previous?.active === true,
      attempts: previous?.attempts ?? 0,
      activationId: activation,
    });
    this.paneScheduler.resume(pane);
    if (this.paneSubscriptions.has(key)) {
      this.subscribeRetryIntents.delete(key);
      if (opts?.active === true) this.queueActivePromotion(pane);
      return;
    }
    if (this.subscribing.has(key) || this.subscribeRetryTimers.has(key)) return;
    this.subscribing.add(key);
    const intent = this.subscribeRetryIntents.get(key)!;
    void this._subscribe(pane, intent.resume, intent.active, intent.activationId);
  }
  onTerminalFrame(fn: TerminalFrameListener): () => void {
    if (this.disposed) return () => {};
    if (!this.terminalTransportUnlisten) {
      this.terminalTransportUnlisten = this.handle.adapter.onTerminalFrame(bytes => {
        if (this.disposed || this.streamPaneRefs.size === 0 || bytes[0] !== 0x13 || bytes.byteLength > terminalHello.maxFrameBytes) return;
        this.terminalFrameListeners.forEach(listener => listener(bytes));
      });
    }
    this.terminalFrameListeners.add(fn);
    return () => {
      this.terminalFrameListeners.delete(fn);
      if (this.terminalFrameListeners.size === 0) {
        this.terminalTransportUnlisten?.();
        this.terminalTransportUnlisten = null;
      }
    };
  }

  activatePane(
    pane: PaneRef,
    opts?: { resume?: boolean; sinceSeq?: number; active?: boolean; activationId?: number },
  ): void {
    const targetKey = paneRefKey(pane);
    for (const [key, current] of [...this.streamPaneRefs]) {
      if (key !== targetKey) this._unsubscribePaneStream(current);
    }
    this.deadPaneKeys.delete(targetKey);
    this.subscribePane(pane, { ...opts, active: true });
  }

  private queueActivePromotion(pane: PaneRef): void {
    const key = paneRefKey(pane);
    if (this.activePaneKey === key && !this.pendingActivePromotion && !this.activePromotionInFlight) return;
    this.activePaneKey = key;
    this.pendingActivePromotion = pane;
    this.flushActivePromotion();
  }

  private flushActivePromotion(): void {
    if (this.activePromotionInFlight || !this.pendingActivePromotion) return;
    const pane = this.pendingActivePromotion;
    this.pendingActivePromotion = null;
    this.activePromotionInFlight = true;
    void Promise.resolve().then(() => this.sendTerminalSubscription(pane, true))
      .catch(() => undefined)
      .finally(() => {
        this.activePromotionInFlight = false;
        this.flushActivePromotion();
      });
  }

  /** Request one bounded canonical
   * resync after local render backpressure shed output. Keep the listener and
   * subscription alive; tearing them down/re-registering would create a second
   * fan-out and could replay stale bytes twice. */
  resyncPane(pane: PaneRef): void {
    const key = paneRefKey(pane);
    if (this.disposed || this.closingPaneKeys.has(key) || this.deadPaneKeys.has(key)) return;
    const activationId = this.terminalActivations.get(key);
    if (activationId !== undefined) {
      void this._invokePane<string>(pane, 'get_terminal_snapshot', {
        paneId: pane.paneId, workspaceId: pane.workspaceId, activationId,
      }).then(encoded => {
        if (this.disposed
          || this.terminalActivations.get(key) !== activationId) return;
        if (typeof encoded !== 'string' || encoded.length > Math.ceil(terminalHello.maxFrameBytes / 3) * 4) throw new Error('invalid terminal snapshot');
        const bytes = Uint8Array.from(atob(encoded), character => character.charCodeAt(0));
        if (bytes[0] !== 0x13) throw new Error('invalid terminal snapshot');
        this.terminalFrameListeners.forEach(fn => fn(bytes));
      }).catch(() => {
        if (this.disposed
          || this.terminalActivations.get(key) !== activationId) return;
        this._failure = { category: 'channel', message: 'terminal snapshot recovery failed' };
        this.setState('error');
      });
      return;
    }
  }

  async fetchTerminalHistory(request: import('@ridge/remote/shared/cloud/signaling').HistoryRequest): Promise<Uint8Array> {
    const encoded = await this._invokePane<string>(request.pane, 'get_terminal_history', { request });
    if (typeof encoded !== 'string' || encoded.length > Math.ceil(terminalHello.maxFrameBytes / 3) * 4) throw new Error('invalid terminal history frame');
    const bytes = Uint8Array.from(atob(encoded), character => character.charCodeAt(0));
    if (bytes[0] !== 0x13) throw new Error('invalid terminal history tag');
    return bytes;
  }

  private _subscribe(pane: PaneRef, _resume = false, active?: boolean, activationId?: number): void {
    const key = paneRefKey(pane);
    try {
      this.sendTerminalSubscription(pane, active === true, activationId);
      this.paneSubscriptions.add(key);
      this.subscribeRetryIntents.delete(key);
      if (active) this.activePaneKey = key;
    } catch {
      const intent = this.subscribeRetryIntents.get(key);
      if (intent) this.scheduleSubscribeRetry(intent);
    } finally { this.subscribing.delete(key); }
  }

  private sendTerminalSubscription(pane: PaneRef, active: boolean, activationId = this.terminalActivations.get(paneRefKey(pane))): void {
    if (!Number.isSafeInteger(activationId) || activationId! < 1) throw new Error('terminal activation required');
    this.handle.adapter.sendControl({ jsonrpc: '2.0', method: 'subscribe-pane', params: { ...pane, active, activationId } });
  }

  private scheduleSubscribeRetry(intent: SubscribeRetryIntent): void {
    const key = paneRefKey(intent.pane);
    if (
      this.disposed
      || this._state !== 'connected'
      || this.closingPaneKeys.has(key)
      || this.deadPaneKeys.has(key)
      || this.paneSubscriptions.has(key)
    ) {
      this.subscribeRetryIntents.delete(key);
      return;
    }
    if (intent.attempts >= SUBSCRIBE_MAX_RETRIES) {
      this.subscribeRetryIntents.delete(key);
      return;
    }
    const next: SubscribeRetryIntent = {
      ...intent,
      attempts: intent.attempts + 1,
    };
    this.subscribeRetryIntents.set(key, next);
    const delay = Math.min(
      SUBSCRIBE_RETRY_MAX_MS,
      SUBSCRIBE_RETRY_BASE_MS * 2 ** (next.attempts - 1),
    );
    const timer = setTimeout(() => {
      this.subscribeRetryTimers.delete(key);
      if (
        this.disposed
        || this._state !== 'connected'
        || this.closingPaneKeys.has(key)
        || this.deadPaneKeys.has(key)
        || this.paneSubscriptions.has(key)
        || this.subscribing.has(key)
      ) return;
      const current = this.subscribeRetryIntents.get(key);
      if (!current) return;
      this.subscribing.add(key);
      void this._subscribe(
        current.pane,
        current.resume,
        current.active,
        current.activationId,
      );
    }, delay);
    this.subscribeRetryTimers.set(key, timer);
  }

  sendStdin(pane: PaneRef, data: TerminalInputData, context: TerminalInputContext): boolean {
    if (!pane.paneId || !data) return false;
    const key = paneRefKey(pane);
    if (this.disposed || this.closingPaneKeys.has(key) || this.deadPaneKeys.has(key)) return false;
    return tryEnqueuePaneInputImmediate(key, () => {
      this.paneScheduler.enqueueInput(pane, data, context);
    });
  }

  enqueueStdinTask(pane: PaneRef, task: () => Promise<TerminalInputData | null> | TerminalInputData | null, context: TerminalInputContext): boolean {
    if (!pane.paneId) return false;
    const key = paneRefKey(pane);
    if (this.disposed || this.closingPaneKeys.has(key) || this.deadPaneKeys.has(key)) return false;
    return tryEnqueuePaneInput(key, async () => {
      const data = await task();
      if (data) this.paneScheduler.enqueueInput(pane, data, context);
    });
  }

  refreshPane(
    pane: PaneRef,
    rows: number,
    cols: number,
    _pixelWidth?: number,
    _pixelHeight?: number,
    owner?: PaneRenderOwner,
  ): void {
    this._resize(pane, rows, cols, owner);
  }
  claimPane(
    pane: PaneRef,
    rows: number,
    cols: number,
    _pixelWidth?: number,
    _pixelHeight?: number,
    owner?: PaneRenderOwner,
  ): Promise<void> {
    this.paneScheduler.resume(pane);
    if (!pane.paneId || rows <= 0 || cols <= 0) return Promise.resolve();
    const key = paneRefKey(pane);
    if (this.disposed || this.closingPaneKeys.has(key) || this.deadPaneKeys.has(key)) {
      return Promise.resolve();
    }
    const params = owner ? { owner } : undefined;
    this._refreshSeq++;
    return this.paneScheduler.scheduleResizeAndWait(
      pane,
      rows,
      cols,
      params,
      { force: true },
    );
  }
  private _resize(pane: PaneRef, rows: number, cols: number, owner?: PaneRenderOwner): void {
    if (!pane.paneId || rows <= 0 || cols <= 0) return;
    const key = paneRefKey(pane);
    if (this.disposed || this.closingPaneKeys.has(key) || this.deadPaneKeys.has(key)) return;
    const params = owner ? { owner } : undefined;
    if (this.paneScheduler.scheduleResize(pane, rows, cols, params, { force: true })) this._refreshSeq++;
  }
  lastRefreshSeq(): number {
    return this._refreshSeq;
  }

  get rpcSchedulingDiagnostics() {
    return this.paneScheduler.diagnostics;
  }

  async createPane(): Promise<string | null> {
    try {
      const workspaceId = this._activeWorkspaceId;
      if (!workspaceId) return null;
      const layout = await this.bridge.invoke<PaneNode>('get_pane_layout_for', { workspaceId });
      const leaves = flattenLeaves(layout);
      if (leaves.length > 0) {
        // Add a terminal to the current workspace by splitting the first leaf — the
        // desktop's own "new terminal" primitive. The mobile renders one pane at a
        // time, so it just shows the new pane; the host sees a split (shared reality).
        const target = { workspaceId, paneId: leaves[0].id };
        const result = await this._invokePane<{ pane_id: string }>(target, 'split_pane', {
          workspaceId,
          paneId: leaves[0].id,
          direction: 'horizontal',
        });
        return result.pane_id || null;
      }
      // Empty workspace: spin up a fresh one and surface its first pane.
      const createdWorkspaceId = await this.bridge.invoke<string>('create_workspace');
      if (!createdWorkspaceId) return null;
      this._activeWorkspaceId = createdWorkspaceId;
      const after = flattenLeaves(await this.bridge.invoke<PaneNode>('get_pane_layout_for', {
        workspaceId: createdWorkspaceId,
      }));
      return after[0]?.id ?? null;
    } catch {
      return null;
    }
  }

  async closePane(pane: PaneRef): Promise<boolean> {
    const key = paneRefKey(pane);
    if (
      !pane.paneId
      || this.disposed
      || this.closingPaneKeys.has(key)
      || this.deadPaneKeys.has(key)
    ) return false;
    this.closingPaneKeys.add(key);
    this.paneScheduler.retire(pane);
    try {
      await this.bridge.invoke('close_pane', {
        paneId: pane.paneId,
        ...(pane.workspaceId ? { workspaceId: pane.workspaceId } : {}),
      });
      this._deactivatePane(key, true);
      void this._refreshPanes();
      return true;
    } catch {
      this.closingPaneKeys.delete(key);
      return false;
    }
  }

  // iter-63：终端类型列表与切换。与 LAN 腿同一对命令、与桌面同一个 host 检测器
  // （`detect_available_shells`），三处不会漂移。
  async listShells(): Promise<RemoteShellInfo[]> {
    const r = await this.bridge.invoke<RemoteShellInfo[]>('detect_available_shells', {});
    return Array.isArray(r) ? r : [];
  }

  async changePaneShell(
    workspaceId: string,
    paneId: string,
    shell: RemoteShellInfo,
  ): Promise<void> {
    const pane = { workspaceId, paneId };
    await this._invokePane(pane, 'change_pane_shell', {
      paneId,
      shell: shell.program,
      args: shell.args ?? [],
    });
    // 重建后必须再激活，否则新 PTY 无订阅者，远端只看到死屏。
    await this._invokePane(pane, 'activate_pane_pty', { workspaceId, paneId });
  }

  // ── workspaces ───────────────────────────────────────────────────────────────
  async listWorkspaces(): Promise<{ workspaces: WorkspaceInfo[] }> {
    const [list, activeId] = await Promise.all([
      // Preserve discovery failures. Returning [] makes a disconnected host
      // look like a healthy host with no workspaces and prevents Query/Hosts
      // from showing actionable progress or retry state.
      this.bridge.invoke<BackendWorkspace[]>('list_workspaces'),
      this.bridge.invoke<string>('get_active_workspace_id').catch(() => this._activeWorkspaceId),
    ]);
    if (activeId) this._activeWorkspaceId = activeId;
    const workspaces = (list ?? []).map((w) => ({
      id: w.id,
      name: w.name ?? undefined,
      active: w.id === activeId,
    }));
    return { workspaces };
  }

  // P1 roster：cloud 侧经 tauriShim invoke → bridge.invoke → allowlist 门控。
  async getTeammateTopology(workspaceId?: string): Promise<TeammateTopology> {
    return this.bridge.invoke<TeammateTopology>(
      'get_teammate_topology',
      workspaceId ? { workspaceId } : {},
    );
  }

  async sendAgentMessage(
    target: AgentMessageTarget,
    message: string,
  ): Promise<AgentMessageReceipt> {
    return this.bridge.invoke<AgentMessageReceipt>('send_agent_message', {
      target_pane_id: target.paneId,
      workspace_id: target.workspaceId,
      agent_id: target.agentId,
      generation: target.generation,
      lease: target.lease,
      message,
      from: 'remote-ui',
      idempotency_key: crypto.randomUUID(),
    });
  }

  async listAgentHistory(workspaceId: string, limit = 24, cursor?: string, query = '', textCursor?: string): Promise<AgentHistoryReply[]> {
    const result = await this.bridge.invoke<unknown>('read_agent_recent_replies', {
      workspaceId,
      limit: Math.max(1, Math.min(100, Math.floor(limit))),
      cursor,
      textCursor,
      query: query.trim(),
    });
    return Array.isArray(result) ? result as AgentHistoryReply[] : [];
  }

  async setTeammateGroups(
    workspaceId: string,
    groups: readonly TeammateGroup[],
    expectedRevision: number,
  ): Promise<TeammateGroupSnapshot> {
    const projection = await this.bridge.invoke<TeammateGroupSnapshot>('set_teammate_groups', { workspaceId, groups, expectedRevision });
    if (!projection || !Array.isArray(projection.groups) || !Number.isSafeInteger(projection.groupsRevision)) throw new Error('Invalid group projection');
    return projection;
  }

  async resumeAgentSession(_workspaceId: string, _agent: string, _sessionId: string, _cwd: string): Promise<string | null> {
    throw new Error('Restore the Agent in its terminal, then reconnect its existing protocol session');
  }

  // P2 阶段 1：脱敏待审批快照（无 action 全文），同 allowlist 门控。
  async listHitlPending(): Promise<HitlPendingItem[]> {
    return this.bridge.invoke<HitlPendingItem[]>('list_hitl_pending', {});
  }

  // P2 阶段 2：远端裁决（nonce 单次消费；仅 approve/reject）。
  async resolveHitlRemote(
    id: string,
    nonce: string,
    verdict: 'approve' | 'reject',
  ): Promise<HitlResolveOutcome> {
    const r = await this.bridge.invoke<{ outcome: HitlResolveOutcome }>('resolve_hitl_remote', {
      id,
      nonce,
      verdict,
    });
    return r?.outcome ?? 'already-resolved';
  }

  async getOrchestrationHealth(): Promise<import('@ridge/remote').OrchestrationHealth> {
    const h = await this.bridge.invoke<{ suspendedAgents?: number; pendingHitl?: number }>(
      'get_orchestration_health',
      {},
    );
    return {
      suspendedAgents: Number(h?.suspendedAgents ?? 0),
      pendingHitl: Number(h?.pendingHitl ?? 0),
    };
  }

  async switchWorkspace(workspaceId: string): Promise<boolean> {
    try {
      await this.bridge.invoke('switch_workspace', { workspaceId });
      this._activeWorkspaceId = workspaceId;
      return true;
    } catch {
      return false;
    }
  }

  async createWorkspace(name?: string): Promise<string | null> {
    // Preserve RPC failures so WorkspaceTree can show the real error instead
    // of silently leaving the create action with no response.
    const id = await this.bridge.invoke<string>('create_workspace', name ? { name } : {});
    return id || null;
  }

  async listSavedWorkspaceFiles(): Promise<
    import('@ridge/remote').SavedWorkspaceFile[]
  > {
    try {
      const raw = await this.bridge.invoke<
        { name?: string; path?: string; mtime_secs?: number; mtimeSecs?: number }[]
      >('list_saved_workspace_files');
      if (!Array.isArray(raw)) return [];
      return raw
        .map((r) => ({
          name: String(r?.name ?? ''),
          path: String(r?.path ?? ''),
          mtimeSecs: Number(r?.mtime_secs ?? r?.mtimeSecs ?? 0),
        }))
        .filter((e) => e.path.length > 0);
    } catch {
      return [];
    }
  }

  async openWorkspaceFromFile(path: string): Promise<string | null> {
    try {
      const id = await this.bridge.invoke<string>('open_workspace_from_file', { path });
      return id || null;
    } catch (e) {
      throw e instanceof Error ? e : new Error(String(e));
    }
  }

  async closeWorkspace(workspaceId: string): Promise<boolean> {
    try {
      await this.bridge.invoke('close_workspace', { workspaceId });
      return true;
    } catch {
      return false;
    }
  }

  async listWorkspacePanes(workspaceId: string): Promise<PaneInfo[]> {
    // Keep pane discovery failures observable. Callers retain the last good
    // snapshot on failure; silently returning [] erases a real remote tree.
    const layout = await this.bridge.invoke<PaneNode>('get_pane_layout_for', { workspaceId });
    return flattenLeaves(layout);
  }

  // ── theme ───────────────────────────────────────────────────────────────────
  // The host's active theme is read in init() (lastTheme → applied by MainApp).
  // Cycling is CONTROL-END-LOCAL (§theme-isolation, like the LAN host's stateless
  // cycle): pick the next theme from the host's catalog and apply its colors to THIS
  // controller only — we do NOT call set_active_theme (that would re-skin the host
  // and every other viewer). MainApp persists the cycled choice as its own override.
  cycleTheme(currentId: string): void {
    void this._cycleTheme(currentId);
  }
  private async _cycleTheme(currentId: string): Promise<void> {
    try {
      const tf = await this.bridge.invoke<{ themes?: ThemeEntryLite[] }>('get_theme_data');
      const themes = (tf?.themes ?? []).filter((t) => t?.id && t.colors);
      if (themes.length === 0) return;
      const cur = themes.findIndex((t) => t.id === currentId);
      const next = themes[(cur + 1) % themes.length];
      const themeType = next.type === 'light' ? 'light' : 'dark';
      this._lastTheme = { id: next.id, themeType, colors: next.colors };
      this.themeListeners.forEach((fn) => fn(next.colors, themeType));
    } catch {
      /* catalog fetch failed — keep current theme */
    }
  }

  // ── misc / parity stubs ────────────────────────────────────────────────────────
  setHostClipboard(_text: string): void {
    // No cloud command for writing the host's system clipboard; best-effort no-op
    // (the LAN path is itself fire-and-forget).
  }
  connect(): void {
    // Cloud boots via cloudControllerBoot in App.svelte, never via this signature.
  }
  getPaneOutput(_pane: PaneRef): string[] {
    return [];
  }
  pruneOutputs(liveIds: Set<string>): void {
    // Release `pty-output` listeners for panes the host no longer reports (bounds
    // listener growth on a long-lived PWA tab — mirrors the LAN pruneOutputs intent).
    const tracked = new Set([
      ...this.paneSubscriptions.keys(),
      ...this.subscribing,
      ...this.paneRpcControllers.keys(),
    ]);
    const schedulerRetired = new Set(this.paneScheduler.prune(liveIds));
    for (const key of tracked) {
      if (!liveIds.has(key)) this._deactivatePane(key, schedulerRetired.has(key));
    }
  }
  send(): void {
    // Raw wsRemote frames have no meaning on the cloud invoke bridge.
  }

  disconnect(): void {
    if (this.disposed) return;
    for (const key of new Set([
      ...this.paneRpcControllers.keys(),
      ...this.paneSubscriptions.keys(),
    ])) retirePaneInput(key);
    this.disposed = true;
    this.terminalTransportUnlisten?.();
    this.terminalTransportUnlisten = null;
    this._failure = null; // 主动断开，清掉失败分级
    this.setState('disconnected');
    this.paneScheduler.dispose();
    this._teardownSubscriptions();
    if (this.treeUnlisten) {
      try { this.treeUnlisten(); } catch { /* already gone */ }
      this.treeUnlisten = null;
    }
    this.groupUnlisten?.();
    this.groupUnlisten = null;
    if (this.metaUnlisten) {
      try { this.metaUnlisten(); } catch { /* already gone */ }
      this.metaUnlisten = null;
    }
    if (this.resizeUnlisten) {
      try { this.resizeUnlisten(); } catch { /* already gone */ }
      this.resizeUnlisten = null;
    }
    this.terminalErrorUnlisten?.();
    this.terminalErrorUnlisten = null;
    // Tear down the WebRTC / E2EE session (idempotent).
    try { this.handle.disconnect(); } catch { /* already torn down */ }
  }
}
