// src/remote/lib/cloudRemote.ts
//
// Cloud control-end transport for the mobile app (design 2026-06-16-mobile-cloud).
//
// The mobile UI (MainApp / BottomTabBar / WorkspaceTree) is written against the
// LAN wsRemote flat protocol ({@link RemoteLink}). The cloud host answerer
// (cloudHostBridge.ts), however, only speaks the Tauri-invoke surface the desktop
// SPA uses — JSON-RPC over the WebRTC E2EE DataChannel, gated by the §5.4 capability
// allow-list and the §4 zero-trust TOTP. That flat protocol is actually `server.rs`
// translating the host's pane-tree/PTY model down for mobile; the cloud bridge does
// NOT do that translation.
//
// So this class RE-DERIVES server.rs's translation on the CLIENT, on top of the
// tauriShim bridge (already attached by cloudControllerBoot). Net effect: the exact
// same mobile UI runs over the secure cloud path with ZERO host changes (the host
// side is the already-shipped desktop binary that desktop-app proves works).
//
// Wire reuse (no new byte path invented — all of this already exists for desktop-app):
//   - invoke(...)                 → bridge.invoke → rpc.request (allow-list gated)
//   - register_pane_delta_channel → core.ts special-cases to bridge.subscribePane
//                                   → 'subscribe-pane' notify → host streams 0x10 pane bytes
//   - listen('pty-output-{ws}-{pane}') → bridge fans the pane bytes (decoded) here
//
// Auth/boot lives in App.svelte (cookie bootstrap → cloudControllerBoot → TOTP gate);
// this class is constructed AFTER the bridge is connected + TOTP-verified, so invoke/
// listen are live. It holds the boot handle so disconnect() tears down the WebRTC.

import { Channel, invoke } from '@tauri-apps/api/core';
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
  type PendingScrollbackPage,
  type ConnectionState,
  type ConnectionFailure,
  type PaneInfo,
  type WorkspaceInfo,
  type WsMessage,
  type RawByteListener,
  type MetaListener,
  type PtyResizeListener,
  type ThemeListener,
  type ThemeSnapshot,
  type TeammateTopology,
  type AgentHistoryReply,
  type TeammateGroup,
  type HitlPendingItem,
  type HitlResolveOutcome,
  type RemoteShellInfo,
} from '@ridge/remote';
import { tryEnqueuePaneInput, retirePaneInput } from '@ridge/remote/shared/terminal/paneInputGate';

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
const INPUT_BATCH_WINDOW_MS = 4;

/**
 * §history-pull（2026-07-02）: 首屏拉取的 scrollback 上限（约 1.5 屏）。host 不再推
 * `RIS + 256KiB` 全量回放；controller 订阅时自己拉这么多作首屏（RIS + tail），再挂 live，
 * 用户向上滚动时才经 `get_pane_scrollback_before` 分批拉更旧历史。16KiB 足够填满移动端
 * 视口 1.5 屏、又小到不会卡死解析器。
 */
const REMOTE_INITIAL_SCROLLBACK_BYTES = 16 * 1024;

/** 滚顶懒加载每批拉取的 scrollback 上限。 */
const REMOTE_OLDER_SCROLLBACK_BYTES = 64 * 1024;

/** One page of scrollback bytes returned by `get_pane_scrollback_tail` / `_before`. */
interface ScrollbackChunk {
  bytes: string;
  start_seq: number;
  end_seq: number;
  at_oldest: boolean;
  head_seq: number;
}

/**
 * §R-CLOUD-CONVERGE — `get_pane_resync_frame` response. `frame` is the COMPLETE
 * host-built reattach frame (RIS + mode-reattach preamble + scrollback tail, via the
 * shared build_resync_frame SSOT); we feed it verbatim. `start_seq`/`at_oldest` seed
 * the scroll-up cursor; `head_seq` is the live/history seam (R-INCR). A superset of
 * ScrollbackChunk — one invoke replaces the old tail+preamble self-assembly.
 */
interface PaneResyncFrame {
  frame: string;
  start_seq: number;
  at_oldest: boolean;
  head_seq: number;
}

/** Flatten a host pane-tree to the mobile's flat leaf list (server.rs's downgrade). */
function flattenLeaves(node: PaneNode | null | undefined): PaneInfo[] {
  if (!node) return [];
  if (node.type === 'leaf') {
    if (!node.id) return []; // pre-hydration placeholder leaf
    // iter-61：agent 标记态随叶子下发（LAN 腿在 build_remote_pane_list 里同源附带）。
    return [{
      id: node.id,
      title: node.title,
      cwd: node.cwd,
      isAgent: node.agent_state === 'busy',
      ...(node.agent_state ? { agentState: node.agent_state } : {}),
      ...(node.agent_id ? { agentId: node.agent_id } : {}),
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
  subscribePane(paneId: string, workspaceId?: string, active?: boolean): Promise<void> | void;
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
  async subscribePane(paneId, workspaceId, active) {
    await invoke('register_pane_delta_channel', {
      workspaceId: workspaceId ?? '',
      paneId,
      active,
      channel: new Channel(),
    });
  },
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
  private paneRpcControllers = new Map<string, AbortController>();
  private closingPaneKeys = new Set<string>();
  private deadPaneKeys = new Set<string>();
  private disposed = false;

  private stateListeners = new Set<(s: ConnectionState) => void>();
  private reconnectListeners = new Set<() => void>();
  private messageListeners = new Set<(msg: WsMessage) => void>();
  private rawByteListeners = new Set<RawByteListener>();
  private metaListeners = new Set<MetaListener>();
  private resizeListeners = new Set<PtyResizeListener>();
  private themeListeners = new Set<ThemeListener>();

  // Per-pane `pty-output-*` unlisten handles (bounded via pruneOutputs / disconnect).
  private ptyUnlisten = new Map<string, UnlistenFn>();
  // Panes whose subscribe is in flight (so concurrent subscribe calls stay idempotent).
  private subscribing = new Set<string>();
  // §history-pull: per-pane seq cursor for lazy "scroll up to load older". Seeded
  // from the initial tail read; advanced by each get_pane_scrollback_before page.
  private scrollbackCursor = new Map<string, { oldestSeq: number; atOldest: boolean }>();
  // Panes whose older-history fetch is in flight (dedup overlapping scroll-up loads).
  private fetchingOlder = new Set<string>();
  // pane-tree-changed unlisten (host-side layout changes → re-list panes).
  private treeUnlisten: UnlistenFn | null = null;
  /** iter-60 G9：pane-meta-changed 退订句柄。 */
  private metaUnlisten: UnlistenFn | null = null;
  // Per-pane decoders are unnecessary: the bridge already decodes bytes→string with a
  // streaming TextDecoder; we re-encode here to feed the byte-oriented mobile canvas.
  private readonly encoder = new TextEncoder();
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
    this.fetchingOlder.delete(key);
    this.scrollbackCursor.delete(key);
    const unlisten = this.ptyUnlisten.get(key);
    if (unlisten) {
      this.ptyUnlisten.delete(key);
      try { unlisten(); } catch { /* already gone */ }
    }
  }

  hasCapability(capability: string): boolean {
    return this.bridge.hasCapability(capability);
  }

  onCapabilitiesChanged(fn: () => void): () => void {
    return this.bridge.onCapabilitiesChanged(fn);
  }

  /**
   * Bring the connection up: read the host's active workspace, watch host-side
   * layout changes, and flip to 'connected'. Must be awaited by App.svelte BEFORE
   * mounting MainApp, since MainApp.onMount calls listPanes()/refreshWorkspaces()
   * which depend on the active workspace id.
   */
  async init(): Promise<void> {
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
      this.metaUnlisten = await this.bridge.listen<{ paneId?: string; title?: string; cwd?: string }>(
        'pane-meta-changed',
        (e) => {
          const p = e.payload;
          if (!p || typeof p.paneId !== 'string') return;
          this.metaListeners.forEach((fn) => fn({
            workspaceId: this._activeWorkspaceId,
            paneId: p.paneId!,
          }, p.title ?? null, p.cwd ?? null));
        },
      );
    } catch {
      /* non-fatal — layout-poll fallback below still refreshes meta */
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
    const mapped: ConnectionState =
      providerState === 'connected' ? 'connected'
      : providerState === 'error' ? 'error'
      : providerState === 'disconnected' ? 'disconnected'
      : 'connecting';
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

  /** Drop all per-pane subscription state — live `pty-output` listeners, in-flight
   *  subscribe flags, and the lazy-scrollback seq cursors. Shared by disconnect()
   *  and the reconnect path so a fresh transport re-subscribes from a clean slate. */
  private _teardownSubscriptions(): void {
    this._abortAllPaneRpcs();
    this.closingPaneKeys.clear();
    this.deadPaneKeys.clear();
    for (const [, unlisten] of this.ptyUnlisten) {
      try { unlisten(); } catch { /* handle points at a dead transport — ignore */ }
    }
    this.ptyUnlisten.clear();
    this.subscribing.clear();
    this.scrollbackCursor.clear();
    this.fetchingOlder.clear();
  }

  private async _handleReconnect(): Promise<void> {
    if (this.disposed || this._reconnecting) return;
    this._reconnecting = true;
    try {
      // Re-authorize. An ICE-restart keeps the host's §4 gate open (verifyTotp is a
      // no-op ok); a full re-handshake re-gates it, so the cached code re-opens it —
      // unless a long outage expired the code's time window, in which case the host
      // rejects and we surface 'error' so the user refreshes for a fresh code.
      let ok = this.fixedAuthorized;
      if (!this.fixedAuthorized && this._verifiedCode) {
        ok = await this.handle.verifyTotp(this._verifiedCode).catch(() => false);
      } else if (!this.fixedAuthorized) {
        // §7.4：本会话经信任授权进入（无缓存 TOTP 码）。full re-handshake 会重置 host 的
        // §4 门，故重连后重跑静默信任握手重新开门；旧 host 无 challenge → 超时 false。
        ok = await this.handle.tryTrustGrant().catch(() => false);
      }
      if (this.disposed) return;
      if (!ok) {
        // 重连后 re-auth 失败：多为长时间断网导致缓存 TOTP 码过期，刷新拿新码即可恢复
        //（非账户/权限问题）→ 归通道类，UI 提示「通道异常」并允许重试/刷新。
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
      if (entry && entry.colors) {
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
    // The cloud host doesn't push host→controller pty-resized; the mobile drives
    // its own size via resize_pane. Registered for parity; effectively unused.
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
    let leaves: PaneInfo[];
    try {
      const layout = await this.bridge.invoke<PaneNode>('get_pane_layout');
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
    opts?: { resume?: boolean; sinceSeq?: number; active?: boolean },
  ): void {
    const { paneId, workspaceId } = pane;
    if (!paneId || !workspaceId) return;
    const key = paneRefKey(pane);
    if (this.disposed || this.closingPaneKeys.has(key) || this.deadPaneKeys.has(key)) return;
    this.paneScheduler.resume(pane);
    if (this.ptyUnlisten.has(key)) {
      if (opts?.active !== undefined) {
        void this.bridge.subscribePane(paneId, workspaceId, opts.active);
      }
      return;
    }
    if (this.subscribing.has(key)) return;
    this.subscribing.add(key);
    void this._subscribe(pane, opts?.resume ?? false, opts?.active);
  }

  /**
   * §history-pull lazy paging: fetch the next older batch of this pane's scrollback
   * (bytes with `seq < cursor`), advancing the seq cursor. Returns the raw bytes to
   * PREPEND above the current scrollback (the caller feeds them through the kernel's
   * prepend path), or `null` when there's nothing to load (already at oldest, no
   * cursor yet, a fetch is already in flight, or the host rejected the command).
   * Idempotent-safe under rapid scroll-up (fetchingOlder dedup + atOldest stop).
   */
  async fetchOlderScrollback(pane: PaneRef): Promise<PendingScrollbackPage | null> {
    const key = paneRefKey(pane);
    const cursor = this.scrollbackCursor.get(key);
    if (!cursor || cursor.atOldest || this.fetchingOlder.has(key)) return null;
    this.fetchingOlder.add(key);
    try {
      const chunk = await this._invokePane<ScrollbackChunk>(pane, 'get_pane_scrollback_before', {
        paneId: pane.paneId,
        workspaceId: pane.workspaceId,
        beforeSeq: cursor.oldestSeq,
        maxBytes: REMOTE_OLDER_SCROLLBACK_BYTES,
      });
      const bytes = chunk.bytes ? this.encoder.encode(chunk.bytes) : new Uint8Array();
      if (!(chunk.start_seq < chunk.end_seq)
          || chunk.end_seq !== cursor.oldestSeq
          || bytes.length === 0) {
        if (bytes.length === 0 && chunk.end_seq === cursor.oldestSeq && chunk.at_oldest) {
          this.scrollbackCursor.set(key, { ...cursor, atOldest: true });
        }
        this.fetchingOlder.delete(key);
        return null;
      }
      let settled = false;
      const finish = () => {
        if (!settled) {
          settled = true;
          this.fetchingOlder.delete(key);
        }
      };
      return {
        bytes,
        startSeq: chunk.start_seq,
        endSeq: chunk.end_seq,
        atOldest: chunk.at_oldest,
        commit: () => {
          if (settled || this.scrollbackCursor.get(key)?.oldestSeq !== chunk.end_seq) return false;
          this.scrollbackCursor.set(key, {
            oldestSeq: chunk.start_seq,
            atOldest: chunk.at_oldest,
          });
          finish();
          return true;
        },
        discard: finish,
      };
    } catch {
      this.fetchingOlder.delete(key);
      return null;
    }
  }

  private async _subscribe(
    pane: PaneRef,
    resume = false,
    active?: boolean,
  ): Promise<void> {
    const { paneId, workspaceId } = pane;
    const key = paneRefKey(pane);
    try {
      // §history-pull（2026-07-02）: the host no longer pushes an on-subscribe
      // `RIS + 256KiB` replay. Pull our own ~1.5-screen tail FIRST and hand it to
      // the kernel (RIS + mode-reattach preamble + tail), THEN wire the live
      // listener. Tail-first (not concurrent) guarantees history is the first chunk
      // the kernel sees and that an idle pane still paints its last screen; the only
      // cost is a tiny gap if the pane is spewing output at the subscribe instant
      // (self-heals on the next redraw). Seed the seq cursor for lazy "scroll up to
      // load older" paging (get_pane_scrollback_before).
      //
      // §keep-alive resume (P4): when the controller kept this pane's mirror kernel
      // ALIVE across a switch, `resume` is true → SKIP the RIS replay entirely (RIS
      // would wipe the surviving kernel down to the tail). The alive kernel keeps its
      // full history and just resumes the live stream below.
      if (!resume) {
        // §R-CLOUD-CONVERGE: the host builds ONE complete resync frame (RIS + active-
        // mode preamble + scrollback tail) via the shared build_resync_frame SSOT — we
        // feed it verbatim, NO client-side assembly. The preamble (?1002h/?1049h/?1006h…)
        // reasserts a TUI's one-time mouse/alt enables that scrolled off the tail, so the
        // mirror kernel comes up mouse-alive (this now actually reaches the cloud gate —
        // the prior get_pane_resync_preamble was never in the real allow-list → dead fix).
        let seeded = false;
        try {
          const rf = await this._invokePane<PaneResyncFrame>(pane, 'get_pane_resync_frame', {
            paneId,
            workspaceId,
            maxBytes: REMOTE_INITIAL_SCROLLBACK_BYTES,
          });
          if (!this.subscribing.has(key)) return; // torn down during the fetch
          this.scrollbackCursor.set(key, { oldestSeq: rf.start_seq, atOldest: rf.at_oldest });
          this.emitRaw(pane, this.encoder.encode(rf.frame));
          seeded = true;
        } catch {
          /* older host (§two-version-line skew: the cloud PWA can ship ahead of the
             user's desktop host, whose allow-list lacks the new command) — fall back
             below to a plain RIS + tail seed (no preamble = the prior shipped cloud
             behavior). Not a regression; the pane still paints its history. */
        }
        if (!seeded) {
          try {
            const chunk = await this._invokePane<ScrollbackChunk>(pane, 'get_pane_scrollback_tail', {
              paneId,
              workspaceId,
              maxBytes: REMOTE_INITIAL_SCROLLBACK_BYTES,
            });
            if (!this.subscribing.has(key)) return;
            this.scrollbackCursor.set(key, {
              oldestSeq: chunk.start_seq,
              atOldest: chunk.at_oldest,
            });
            this.emitRaw(pane, this.encoder.encode('\x1bc' + (chunk.bytes ?? '')));
          } catch {
            // Older host / command rejected: no seeded history — degrade to "first live
            // frame acts as the replay".
          }
        }
        if (!this.subscribing.has(key)) return;
      }

      // Per-pane `pty-output-{ws}-{pane}` event. The bridge keys its dispatch on the
      // trailing pane UUID only, so the ws segment is cosmetic — but we use the real
      // active ws for fidelity. Payload arrives as decoded `{data}`; re-encode to bytes.
      const unlisten = await this.bridge.listen<{ data: string }>(
        `pty-output-${workspaceId}-${paneId}`,
        (e) => {
          const bytes = this.encoder.encode(e.payload?.data ?? '');
          if (bytes.length) this.emitRaw(pane, bytes);
        },
      );
      // Idempotency guard: a teardown between listen() awaiting and resolving.
      if (!this.subscribing.has(key)) {
        unlisten();
        return;
      }
      this.ptyUnlisten.set(key, unlisten);
      // Tell the host to start streaming (core.ts maps this to bridge.subscribePane →
      // 'subscribe-pane' notify; the Channel arg is ignored in the browser shim).
      await this.bridge.subscribePane(paneId, workspaceId, active);
    } catch {
      /* subscribe failed — pane stays blank; a later refresh/re-subscribe retries */
    } finally {
      this.subscribing.delete(key);
    }
  }

  sendStdin(pane: PaneRef, data: string): boolean {
    if (!pane.paneId || !data) return false;
    const key = paneRefKey(pane);
    if (this.disposed || this.closingPaneKeys.has(key) || this.deadPaneKeys.has(key)) return false;
    return tryEnqueuePaneInput(key, () => {
      this.paneScheduler.enqueueInput(pane, data);
    });
  }

  enqueueStdinTask(pane: PaneRef, task: () => Promise<string | null> | string | null): boolean {
    if (!pane.paneId) return false;
    const key = paneRefKey(pane);
    if (this.disposed || this.closingPaneKeys.has(key) || this.deadPaneKeys.has(key)) return false;
    return tryEnqueuePaneInput(key, async () => {
      const data = await task();
      if (data) this.paneScheduler.enqueueInput(pane, data);
    });
  }

  refreshPane(pane: PaneRef, rows: number, cols: number, _pixelWidth?: number, _pixelHeight?: number): void {
    this._resize(pane, rows, cols);
  }
  claimPane(pane: PaneRef, rows: number, cols: number, _pixelWidth?: number, _pixelHeight?: number): void {
    this.paneScheduler.resume(pane);
    this._resize(pane, rows, cols);
  }
  private _resize(pane: PaneRef, rows: number, cols: number): void {
    if (!pane.paneId || rows <= 0 || cols <= 0) return;
    const key = paneRefKey(pane);
    if (this.disposed || this.closingPaneKeys.has(key) || this.deadPaneKeys.has(key)) return;
    if (this.paneScheduler.scheduleResize(pane, rows, cols)) this._refreshSeq++;
  }
  lastRefreshSeq(): number {
    return this._refreshSeq;
  }

  get rpcSchedulingDiagnostics() {
    return this.paneScheduler.diagnostics;
  }

  async createPane(): Promise<string | null> {
    try {
      const layout = await this.bridge.invoke<PaneNode>('get_pane_layout');
      const leaves = flattenLeaves(layout);
      if (leaves.length > 0) {
        // Add a terminal to the current workspace by splitting the first leaf — the
        // desktop's own "new terminal" primitive. The mobile renders one pane at a
        // time, so it just shows the new pane; the host sees a split (shared reality).
        const target = { workspaceId: this._activeWorkspaceId, paneId: leaves[0].id };
        const result = await this._invokePane<{ pane_id: string }>(target, 'split_pane', {
          paneId: leaves[0].id,
          direction: 'horizontal',
        });
        return result.pane_id || null;
      }
      // Empty workspace: spin up a fresh one and surface its first pane.
      await this.bridge.invoke<string>('create_workspace');
      const after = flattenLeaves(await this.bridge.invoke<PaneNode>('get_pane_layout'));
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

  // iter-61：标记 / 取消标记某 pane 为 agent（工作区弹层终端项的标记按钮）。
  // 与 LAN 腿同一对命令；老 host 的 allowlist 无此项 → invoke 抛错，UI 提示不静默。
  async markPaneAgent(
    workspaceId: string,
    paneId: string,
    on: boolean,
    agentId?: string,
  ): Promise<void> {
    if (on) {
      await this._invokePane({ workspaceId, paneId }, 'register_teammate_agent', {
        workspaceId,
        paneId,
        agentId: agentId || 'agent',
      });
    } else {
      await this._invokePane(
        { workspaceId, paneId },
        'release_teammate_agent',
        { workspaceId, paneId },
      );
    }
    void this._refreshPanes();
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

  async listAgentHistory(limit = 24): Promise<AgentHistoryReply[]> {
    const result = await this.bridge.invoke<unknown>('read_agent_recent_replies', {
      projectPaths: [],
      limit: Math.max(1, Math.min(100, Math.floor(limit))),
    });
    return Array.isArray(result) ? result as AgentHistoryReply[] : [];
  }

  async setTeammateGroups(
    workspaceId: string,
    groups: readonly TeammateGroup[],
  ): Promise<void> {
    await this.bridge.invoke('set_teammate_groups', { workspaceId, groups });
  }

  async resumeAgentSession(
    workspaceId: string,
    agent: string,
    sessionId: string,
    cwd: string,
  ): Promise<string | null> {
    const result = await this.bridge.invoke<{ paneId?: unknown }>('resume_agent_session', {
      workspaceId,
      agent,
      sessionId,
      cwd,
    });
    return typeof result?.paneId === 'string' && result.paneId.length > 0
      ? result.paneId
      : null;
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
    try {
      const id = await this.bridge.invoke<string>('create_workspace', name ? { name } : {});
      return id || null;
    } catch {
      return null;
    }
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
      const themes = (tf?.themes ?? []).filter((t) => t && t.id && t.colors);
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
      ...this.ptyUnlisten.keys(),
      ...this.subscribing,
      ...this.scrollbackCursor.keys(),
      ...this.fetchingOlder,
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

  private emitRaw(pane: PaneRef, bytes: Uint8Array): void {
    this.rawByteListeners.forEach((fn) => fn(pane, bytes));
  }

  disconnect(): void {
    if (this.disposed) return;
    for (const key of new Set([
      ...this.paneRpcControllers.keys(),
      ...this.scrollbackCursor.keys(),
      ...this.ptyUnlisten.keys(),
    ])) retirePaneInput(key);
    this.disposed = true;
    this._failure = null; // 主动断开，清掉失败分级
    this.setState('disconnected');
    this.paneScheduler.dispose();
    this._teardownSubscriptions();
    if (this.treeUnlisten) {
      try { this.treeUnlisten(); } catch { /* already gone */ }
      this.treeUnlisten = null;
    }
    if (this.metaUnlisten) {
      try { this.metaUnlisten(); } catch { /* already gone */ }
      this.metaUnlisten = null;
    }
    // Tear down the WebRTC / E2EE session (idempotent).
    try { this.handle.disconnect(); } catch { /* already torn down */ }
  }
}
