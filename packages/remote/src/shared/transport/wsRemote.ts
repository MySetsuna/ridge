import { getRemoteDeviceId } from './deviceId';

export type ConnectionState = 'connecting' | 'connected' | 'disconnected' | 'error';

// ── 连接失败分级（任务 A）───────────────────────────────────────────────────
// 服务端 WS 在「已认证但无权」时会先升级、下发一帧 `{t:"error",code,message}`，再以
// close code 4403 关闭；匿名/伪造则是不透明 403（连升级都不给）。客户端据此把失败分成
// 两类，让 UI 能区分处置：
//   - 'user'    用户问题：账户/设备归属/权限不匹配（USERNAME_MISMATCH /
//               DEVICE_NOT_OWNED / DEVICE_TOKEN_MISMATCH / NOT_PREMIUM）。无法靠重试
//               解决，应退回登录/用户主页换凭据，绝不无限 pending。
//   - 'parked'  设备已停用（DEVICE_PARKED）：去控制台启用或升级，单列以便专门文案。
//   - 'channel' 通道问题：信令/WebRTC/网络/并发超限（TOO_MANY_CONNECTIONS），可重试。
export type ConnectionFailureCategory = 'user' | 'parked' | 'channel';

export interface ConnectionFailure {
  category: ConnectionFailureCategory;
  /** 服务端稳定错误码（契约 §2），通道类网络故障无码时为 undefined。 */
  code?: string;
  /** 人类可读信息（来自服务端 error 帧或本地兜底）。 */
  message?: string;
}

/** WS close code：服务端「已认证但无权」专用关闭码（升级后下发 error 帧再以此关闭）。 */
export const WS_CLOSE_AUTHENTICATED_FORBIDDEN = 4403;

/** 归类为「用户问题」的稳定错误码（退回登录，不重试）。 */
const USER_FAILURE_CODES = new Set([
  'USERNAME_MISMATCH',
  'DEVICE_NOT_OWNED',
  'DEVICE_TOKEN_MISMATCH',
  'NOT_PREMIUM',
]);

/**
 * 把一帧 error 的 code（可选）+ close code 映射成失败分级。
 *   - 命中用户类 code → 'user'
 *   - DEVICE_PARKED → 'parked'
 *   - 其余（含无 code 的网络/信令断、TOO_MANY_CONNECTIONS）→ 'channel'
 * 注：close code 4403 仅表示「已认证但无权」，真正的细分以同时下发的 error 帧 code 为准；
 * 若只拿到 4403 而没拿到 code（理论上不应发生），按用户类处理（不无限重试）。
 */
export function classifyFailure(code?: string, closeCode?: number): ConnectionFailure {
  if (code === 'DEVICE_PARKED') return { category: 'parked', code };
  if (code && USER_FAILURE_CODES.has(code)) return { category: 'user', code };
  if (!code && closeCode === WS_CLOSE_AUTHENTICATED_FORBIDDEN) {
    return { category: 'user', code: undefined };
  }
  return { category: 'channel', code };
}

function uuidFromBytes(bytes: Uint8Array, offset: number = 0): string {
  const hex: string[] = [];
  for (let i = offset; i < offset + 16; i++) {
    hex.push(bytes[i].toString(16).padStart(2, '0'));
  }
  const h = hex.join('');
  return `${h.slice(0,8)}-${h.slice(8,12)}-${h.slice(12,16)}-${h.slice(16,20)}-${h.slice(20)}`;
}

export type RawByteListener = (paneId: string, data: Uint8Array) => void;
export type MetaListener = (paneId: string, title: string | null, cwd: string | null) => void;
export type PtyResizeListener = (paneId: string, rows: number, cols: number) => void;
export type ThemeListener = (colors: Record<string, string>, themeType: 'dark' | 'light') => void;

// Keep for backward compat — consumers should migrate to onRawBytes.
export type BinaryDeltaListener = RawByteListener;

const MAX_PANE_OUTPUT_LINES = 5000;

// ── Message queue for buffering during reconnect ──
// If the queue exceeds this many messages, we reload the page to avoid
// stale state buildup (the reconnect would replay too much history).
const MAX_QUEUED_MESSAGES = 50;

// ── Connection liveness tuning ──
// Mobile browsers silently drop the socket when the tab is backgrounded, often
// without delivering a timely `close`. A heartbeat detects the half-open socket;
// exponential backoff + a foreground liveness probe recover from it.
const HEARTBEAT_INTERVAL_MS = 15_000;
const PONG_TIMEOUT_MS = 10_000;
// Snappier deadline when we re-probe on foreground/online — we want to notice a
// dead socket fast so the reconnect feels instant when the user returns.
const LIVENESS_PROBE_TIMEOUT_MS = 4_000;
const RECONNECT_BASE_MS = 1_000;
const RECONNECT_MAX_MS = 15_000;

export interface PaneInfo {
  id: string;
  title?: string;
  cwd?: string;
  /** iter-61：该 pane 是否已标记为 agent（工作区弹层标记按钮的当前态）。 */
  isAgent?: boolean;
}

/**
 * 在途请求的键。**必须带 `_reqId`**（iter-63 手机端 e2e 实证）：
 * 旧实现只按 `responseType` 作键，于是任何两条并发 `invoke-request` 都注册在
 * `'invoke-result'` 这一个键上，后者 `set` 直接顶掉前者——前一条永远等不到回包，
 * 5s 后超时抛错；活下来那条还可能收到**另一条命令**的结果。手机端花名册正是
 * `Promise.all([topology, hitlPending, health])` 三连发，于是恒定失败、只显示一个「—」，
 * 而后端数据一直是对的（数据面直测 roster 完好）。
 *
 * 无 `_reqId` 的老式请求（list-workspaces 等）保持按类型作键，行为逐字不变。
 */
export function pendingKey(responseType: string, reqId: unknown): string {
  return typeof reqId === 'number' ? `${responseType}#${reqId}` : responseType;
}

/** host `detect_available_shells` 的一条（与桌面 `ShellInfo` 同形）。 */
export interface RemoteShellInfo {
  id: string;
  label: string;
  program: string;
  args: string[];
}

export interface WorkspaceInfo {
  id: string;
  name?: string;
  active: boolean;
}

export interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
  is_ignored?: boolean | null;
  child_count?: number;
}

export interface GitStatus {
  staged: string[];
  unstaged: { name: string; status: string }[];
  commits: { msg: string; hash: string; time: string }[];
}

export type WsMessage = {
  type: 'panes';
  panes: PaneInfo[];
  /** Iteration 63: identifies cached snapshots pushed for non-active workspaces. */
  workspaceId?: string;
} | {
  type: 'output';
  paneId: string;
  data: string;
} | {
  type: 'delta';
  paneId: string;
  data: string;
} | {
  type: 'pty-meta';
  paneId: string;
  title: string | null;
  cwd: string | null;
} | {
  type: 'pty-resized';
  paneId: string;
  rows: number;
  cols: number;
} | {
  type: 'files';
  path: string;
  entries: FileEntry[];
} | {
  type: 'git-status';
  staged: string[];
  unstaged: { name: string; status: string }[];
  commits: { msg: string; hash: string; time: string }[];
} | {
  type: 'error';
  message: string;
} | {
  type: 'workspaces';
  workspaces: WorkspaceInfo[];
} | {
  type: 'current-project';
  path: string;
} | {
  type: 'switch-workspace-result';
  success: boolean;
  workspaceId?: string;
  error?: string;
} | {
  type: 'create-workspace-result';
  success: boolean;
  workspaceId?: string;
} | {
  type: 'create-pane-result';
  success: boolean;
  paneId?: string;
  error?: string;
} | {
  type: 'close-pane-result';
  success: boolean;
  error?: string;
} | {
  type: 'close-workspace-result';
  success: boolean;
  error?: string;
} | {
  type: 'workspace-renamed';
  workspaceId: string;
  name: string;
} | {
  type: 'theme';
  themeType: 'dark' | 'light';
  colors: Record<string, string>;
};

type Listener = (msg: WsMessage) => void;

/** Cached theme snapshot shape ({@link RemoteLink.lastTheme}). */
export interface ThemeSnapshot {
  id?: string;
  themeType: 'dark' | 'light';
  colors: Record<string, string>;
}

/**
 * Control-end transport surface consumed by the mobile UI (App / AuthScreen /
 * MainApp / BottomTabBar / WorkspaceTree). Two implementations:
 *   - {@link RemoteConnection} — LAN WebSocket (wsRemote protocol, self-signed TLS).
 *   - `CloudRemoteConnection` (cloudRemote.ts) — cloud WebRTC E2EE + zero-trust,
 *     translating these calls onto the Tauri-invoke bridge (server.rs's flat
 *     protocol re-derived on the client).
 * Typing the UI against this interface (not the concrete class) is what lets the
 * exact same mobile UI ride either transport — see design 2026-06-16-mobile-cloud.
 */
/** P1 teammate roster 成员（`get_teammate_topology` 投影；无 MCP endpoint/token）。 */
export interface TeammateRosterMember {
  id: string;
  name: string;
  paneId: string;
  paneIndex: number | null;
  role: string;
  status: string;
  capability?: unknown;
  /** iter-62：由「该分屏下真跑着 agent CLI」自动识别入册（而非人工标记）。 */
  isAuto?: boolean;
  /** iter-62：终端近 12s 是否还在吐字（`working` / `idle`）。 */
  activity?: string;
  /** iter-62：该 pane 输出的单调流水号。 */
  outputSeq?: number;
  /** iter-62：近期回复——pane 尾部输出剥 ANSI 的最后几行，随快照下发，
   *  手机端因此不必为每个成员各发一次 RPC。 */
  recentOutput?: string;
}

/** P1 teammate 拓扑快照（roster + leader；edges 预留）。 */
export interface TeammateTopology {
  roster: TeammateRosterMember[];
  leaderId: string | null;
  edges: unknown[];
}

/** P2：待裁决高危动作的脱敏快照——绝不含 action 命令全文。 */
export interface HitlPendingItem {
  id: string;
  initiator: string;
  level: string;
  reason: string;
  createdAt: number;
  /** 阶段 2：一次性裁决票据（E2EE 信道内下发；裁决时回传，host 恒时比对+单次消费）。 */
  resolutionNonce: string;
}

/** P2 阶段 2：远端裁决结局。 */
export type HitlResolveOutcome =
  | 'consumed'
  | 'already-resolved'
  | 'nonce-mismatch'
  | 'bad-verdict';

/** R19：编排健康只读快照（与桌面 get_orchestration_health 同源）。 */
export interface OrchestrationHealth {
  suspendedAgents: number;
  pendingHitl: number;
}

export interface RemoteLink {
  state(): ConnectionState;
  /**
   * 最近一次进入 'error' 的失败详情（分级 + 服务端 code）。UI 据此区分「用户问题
   * （退回登录）」「设备停用」「通道异常（可重试）」。无失败或已恢复时返回 null。
   */
  lastFailure(): ConnectionFailure | null;
  onStateChange(fn: (s: ConnectionState) => void): () => void;
  /** Negotiated coarse host capability; LAN legacy hosts expose the full UI set. */
  hasCapability(capability: string): boolean;
  /** Fires after capability negotiation changes (initial hello and reconnects). */
  onCapabilitiesChanged(fn: () => void): () => void;
  onReconnect(fn: () => void): () => void;
  onMessage(fn: Listener): () => void;
  onRawBytes(fn: RawByteListener): () => void;
  onMetadata(fn: MetaListener): () => void;
  onPtyResize(fn: PtyResizeListener): () => void;
  onTheme(fn: ThemeListener): () => void;
  lastTheme(): ThemeSnapshot | null;
  cycleTheme(currentId: string): void;
  setHostClipboard(text: string): void;
  /** LAN-only signature; the cloud impl ignores it (it boots via cloudControllerBoot). */
  connect(
    host: string,
    port: number,
    auth?: string,
    authType?: 'code' | 'token',
    secure?: boolean,
  ): void;
  getPaneOutput(paneId: string): string[];
  pruneOutputs(liveIds: Set<string>): void;
  send(msg: Record<string, unknown>): void;
  listPanes(): void;
  // §keep-alive resume: pass `{ resume: true }` when the controller kept this
  // pane's mirror kernel alive (mobile P4 keep-alive) and is re-subscribing after
  // a switch — the host then skips the RIS-bearing resync that would wipe the live
  // kernel, and just resumes the live stream. `sinceSeq` (forward-compat) requests
  // an incremental gap replay from that byte cursor instead. Omit both for a fresh
  // subscribe (full RIS + scrollback + mode reattach).
  subscribePane(
    paneId: string,
    opts?: {
      resume?: boolean;
      sinceSeq?: number;
      workspaceId?: string;
      /** Foreground pane owns the transport's reserved priority lane. */
      active?: boolean;
    },
  ): void;
  /**
   * §history-pull（cloud-only）: fetch the next older batch of a pane's scrollback
   * (seq-cursor paging via get_pane_scrollback_before) to PREPEND above the current
   * buffer when the viewport nears the top. Returns the raw bytes, or null when
   * there's nothing more to load. The LAN link omits it (optional) → no-op there.
   */
  fetchOlderScrollback?(paneId: string): Promise<PendingScrollbackPage | null>;
  /**
   * iter-61：把某 pane 标记 / 取消标记为 agent（远端工作区弹层的标记按钮）。
   * 两条腿（LAN invoke-request / cloud RPC）都实现；老 host 会以错误拒绝，UI 提示即可。
   */
  markPaneAgent?(workspaceId: string, paneId: string, on: boolean, agentId?: string): Promise<void>;
  /**
   * iter-63：手机端切终端类型（PS → WSL / Git Bash …）。
   * 列表与桌面**同源**——都是 host 的 `detect_available_shells`，不在客户端另攒一份，
   * 否则两端会漂移（用户明确要求「终端类型列表对齐桌面端」）。
   */
  listShells?(): Promise<RemoteShellInfo[]>;
  /** 原地换 shell：拆该 pane 的 PTY → 按新 program/args 重建 → 重新激活。 */
  changePaneShell?(workspaceId: string, paneId: string, shell: RemoteShellInfo): Promise<void>;
  sendStdin(paneId: string, data: string): void;
  refreshPane(paneId: string, rows: number, cols: number, pixelWidth: number, pixelHeight: number): void;
  claimPane(paneId: string, rows: number, cols: number, pixelWidth: number, pixelHeight: number): void;
  lastRefreshSeq(): number;
  listWorkspaces(): Promise<{ workspaces: WorkspaceInfo[] }>;
  /** P1 roster：只读拓扑快照（capability `teammate` 协商后可用；UI 轮询取数）。 */
  getTeammateTopology(workspaceId?: string): Promise<TeammateTopology>;
  listHitlPending(): Promise<HitlPendingItem[]>;
  resolveHitlRemote(
    id: string,
    nonce: string,
    verdict: 'approve' | 'reject',
  ): Promise<HitlResolveOutcome>;
  /** R19：suspended / pending 计数（allowlist 只读）。 */
  getOrchestrationHealth(): Promise<OrchestrationHealth>;
  switchWorkspace(workspaceId: string): Promise<boolean>;
  createWorkspace(name?: string): Promise<string | null>;
  renameWorkspace?(workspaceId: string, name: string): Promise<boolean>;
  saveWorkspace?(workspaceId: string, name: string): Promise<boolean>;
  createPane(shell?: string): Promise<string | null>;
  closePane(paneId: string): Promise<boolean>;
  closeWorkspace(workspaceId: string): Promise<boolean>;
  listWorkspacePanes(workspaceId: string): Promise<PaneInfo[]>;
  /** Host `~/ridge-workspaces/*.ridge` inventory (open-only on mobile; no manage). */
  listSavedWorkspaceFiles(): Promise<SavedWorkspaceFile[]>;
  /** Open a .ridge path on the host into a live workspace; returns workspace id. */
  openWorkspaceFromFile(path: string): Promise<string | null>;
  disconnect(): void;
}

/** Host-disk saved workspace file (list_saved_workspace_files). */
export interface SavedWorkspaceFile {
  name: string;
  path: string;
  /** Seconds since epoch (host FS mtime). */
  mtimeSecs: number;
}

/** A scrollback page whose cursor advances only after kernel prepend succeeds. */
export interface PendingScrollbackPage {
  bytes: Uint8Array;
  startSeq: number;
  endSeq: number;
  atOldest: boolean;
  commit(): boolean;
  discard(): void;
}

export function remoteWebSocketUrl(input: {
  host: string;
  port: number;
  auth: string;
  authType: 'code' | 'token';
  device: string;
  secure: boolean;
}): string {
  const scheme = input.secure ? 'wss' : 'ws';
  return `${scheme}://${input.host}:${input.port}/ws?${input.authType}=${encodeURIComponent(input.auth)}&device=${encodeURIComponent(input.device)}`;
}

export class RemoteConnection implements RemoteLink {
  private ws: WebSocket | null = null;
  private stateListeners: Set<(s: ConnectionState) => void> = new Set();
  private messageListeners: Set<Listener> = new Set();
  private binaryDeltaListeners: Set<BinaryDeltaListener> = new Set();
  private rawByteListeners: Set<RawByteListener> = new Set();
  private metaListeners: Set<MetaListener> = new Set();
  private resizeListeners: Set<PtyResizeListener> = new Set();
  private themeListeners: Set<ThemeListener> = new Set();
  private _lastTheme: { id?: string; themeType: 'dark' | 'light'; colors: Record<string, string> } | null = null;
  private _state: ConnectionState = 'disconnected';
  // 最近一次失败分级（任务 A 问题1）。进入 'error' 时填充，恢复/重连时清空。
  private _failure: ConnectionFailure | null = null;
  // 服务端升级后下发的 `{t:"error",code}` 暂存：onclose(4403) 紧随其后，用它做精确分级。
  private _pendingServerError: { code?: string; message?: string } | null = null;
  private paneOutputs: Map<string, string[]> = new Map();
  private _pendingRequests: Map<string, { resolve: (v: unknown) => void; reject: (e: Error) => void }> = new Map();
  private _reqCounter = 0;
  private _refreshSeq = 0;
  private _host: string = '';
  private _port: number = 0;
  private _token: string = '';
  private _authType: 'code' | 'token' = 'code';
  private _secure: boolean | null = null;

  // ── Reconnect / heartbeat state ──
  private _intentionalClose = false;
  private _reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private _reconnectAttempts = 0;
  private _heartbeatTimer: ReturnType<typeof setInterval> | null = null;
  private _pongDeadline: ReturnType<typeof setTimeout> | null = null;
  private _hasConnectedOnce = false;
  private reconnectListeners: Set<() => void> = new Set();
  private _windowListenersAttached = false;
  private _onVisibility: (() => void) | null = null;
  private _onOnline: (() => void) | null = null;
  private _onForeground: (() => void) | null = null;

  // ── Message queue for buffering during disconnect ──
  private _messageQueue: WsMessage[] = [];
  private _isReconnecting = false;

  // ── §history-pull（LAN 对齐 cloudRemote）──
  // 每 pane 的 seq 游标：订阅时由 host 的 `scrollback-meta` 帧播种（首屏 tail 的最旧字节），
  // 用户滚顶时经 `scrollback-before` 分批向更旧推进。atOldest 后停止分页。
  private scrollbackCursor = new Map<string, { oldestSeq: number; atOldest: boolean }>();
  private paneWorkspaces = new Map<string, string>();
  // 正在拉取更旧历史的 pane（去重快速连续的滚顶加载）。
  private fetchingOlder = new Set<string>();

  // ── §perf: three-segment latency instrumentation (B 方案诊断埋点) ──
  // All marks are performance.now() (ms, monotonic). Mirrors the server's
  // `ridge::remote::perf` trace so a slow link can be split into upgrade /
  // connect / first-byte segments and read back via getPerf().
  private _perf: {
    connectStart: number | null;   // _open() called (socket construction)
    upgradeStart: number | null;   // ws.onopen fired (≈ server ws_upgrade)
    firstFrame: number | null;     // first message received (text or binary)
    firstPtyBytes: number | null;  // first onRawBytes dispatch
  } = { connectStart: null, upgradeStart: null, firstFrame: null, firstPtyBytes: null };

  state() { return this._state; }
  lastFailure() { return this._failure; }
  hasCapability(_capability: string) { return true; }
  onCapabilitiesChanged(fn: () => void) {
    fn();
    return () => {};
  }

  /** 进入 'error' 终态并记录失败分级（供 UI 区分用户问题 / 通道异常 / 设备停用）。 */
  private failWith(failure: ConnectionFailure) {
    this._failure = failure;
    this.setState('error');
  }

  onStateChange(fn: (s: ConnectionState) => void) {
    this.stateListeners.add(fn);
    return () => this.stateListeners.delete(fn);
  }

  /** Fires when the socket comes back up *after* a previous drop (not the first
   *  connect). Consumers use this to re-subscribe panes and resync UI state —
   *  the reconnect opens a brand-new server-side socket with no subscriptions. */
  onReconnect(fn: () => void) {
    this.reconnectListeners.add(fn);
    return () => this.reconnectListeners.delete(fn);
  }

  onMessage(fn: Listener) {
    this.messageListeners.add(fn);
    return () => this.messageListeners.delete(fn);
  }

  onBinaryDelta(fn: BinaryDeltaListener) {
    this.binaryDeltaListeners.add(fn);
    return () => this.binaryDeltaListeners.delete(fn);
  }

  onRawBytes(fn: RawByteListener) {
    this.rawByteListeners.add(fn);
    return () => this.rawByteListeners.delete(fn);
  }

  onMetadata(fn: MetaListener) {
    this.metaListeners.add(fn);
    return () => this.metaListeners.delete(fn);
  }

  onPtyResize(fn: PtyResizeListener) {
    this.resizeListeners.add(fn);
    return () => this.resizeListeners.delete(fn);
  }

  /** Theme push from the desktop (sent at connect, cached so a late subscriber
   *  — e.g. MainApp mounting after auth — can still read it via lastTheme()). */
  onTheme(fn: ThemeListener) {
    this.themeListeners.add(fn);
    return () => this.themeListeners.delete(fn);
  }
  lastTheme() { return this._lastTheme; }

  /** Ask the host to cycle to the theme *after* `currentId` and push it back as
   *  a `theme` message (applied via onTheme). Stateless on the host — it never
   *  writes the active theme to disk nor clobbers peers (§theme-isolation): the
   *  control end owns its own appearance. Pass the id the client currently shows
   *  (from lastTheme()) so the host can compute the next one. */
  cycleTheme(currentId: string) {
    this.send({ type: 'cycle-theme', current: currentId });
  }

  /** Mirror a copied selection onto the DESKTOP host's system clipboard so the
   *  host's own native paste (Ctrl+V) picks it up — the control end's copy
   *  writes BOTH its local clipboard and the host's. Best-effort / fire-and-forget. */
  setHostClipboard(text: string) {
    if (text) this.send({ type: 'set-host-clipboard', text });
  }

  /** §perf: shallow snapshot of the three-segment latency marks
   *  (performance.now() ms, monotonic) for the current connection cycle.
   *  Mirrors the server's `ridge::remote::perf` trace. */
  getPerf() { return { ...this._perf }; }

  connect(
    host: string,
    port: number,
    auth?: string,
    authType: 'code' | 'token' = 'code',
    secure?: boolean,
  ) {
    if (!auth) { this.failWith({ category: 'channel', message: 'missing credential' }); return; }
    this._clearReconnectTimer();
    this._intentionalClose = false;
    this._reconnectAttempts = 0;
    this._host = host;
    this._port = port;
    this._token = auth;
    this._authType = authType;
    this._secure = secure ?? null;
    this._attachWindowListeners();
    this._open();
  }

  /** (Re)open the socket using the stored host/port/token. All connect attempts
   *  — first and reconnect — funnel through here so they share heartbeat,
   *  resync, and backoff handling. */
  private _open() {
    if (this.ws) {
      // Detach handlers so the old socket's close can't trigger a reconnect.
      this.ws.onopen = this.ws.onclose = this.ws.onerror = this.ws.onmessage = null;
      try { this.ws.close(); } catch { /* already closing */ }
      this.ws = null;
    }
    this.setState('connecting');
    // §perf: start a fresh measurement window for this connection attempt
    // (first connect and every reconnect both funnel through _open).
    this._perf = { connectStart: performance.now(), upgradeStart: null, firstFrame: null, firstPtyBytes: null };
    // Match the page's scheme: an HTTPS-served page must use wss:// (mixed
    // content blocks ws:// from https://). TLS is what unlocks WebGPU on the
    // LAN, so this is the common path in production.
    const secure = this._secure ?? location.protocol === 'https:';
    // §L-3: pin the session to this device (in addition to its source IP) so a
    // token replayed from another device behind the same NAT egress can't
    // connect. MUST match the `device` sent to /verify at issuance.
    const url = remoteWebSocketUrl({
      host: this._host,
      port: this._port,
      auth: this._token,
      authType: this._authType,
      device: getRemoteDeviceId(),
      secure,
    });
    const ws = new WebSocket(url);
    this.ws = ws;
    ws.binaryType = 'arraybuffer';

    ws.onopen = () => {
      // §perf: onopen ≈ server-side ws_upgrade — stamp it and log the client's
      // view of the connect/upgrade latency (connectStart → onopen).
      this._perf.upgradeStart = performance.now();
      console.log('[remote-perf] upgrade', {
        sinceConnectMs: this._perf.connectStart != null
          ? Math.round(this._perf.upgradeStart - this._perf.connectStart)
          : null,
      });
      this._reconnectAttempts = 0;
      this._startHeartbeat();
      this.setState('connected');
      // A reopen after the first successful connect is a genuine reconnect — the
      // server socket is fresh and holds no pane subscriptions, so consumers
      // must resync. The first connect is wired by the page's own onMount.
      if (this._hasConnectedOnce) {
        this.reconnectListeners.forEach(fn => { try { fn(); } catch { /* listener owns its errors */ } });
        // Flush any messages queued during disconnect.
        this._flushQueue();
      }
      this._hasConnectedOnce = true;
    };
    ws.onclose = (ev) => this._handleClose(ev.code);
    ws.onerror = () => this._handleDrop();
    ws.onmessage = (event) => this._handleMessage(event);
  }

  private _handleMessage(event: MessageEvent) {
    // §perf: stamp the first inbound frame (text or binary) of this cycle.
    if (this._perf.firstFrame == null) this._perf.firstFrame = performance.now();
    // Any inbound byte proves the socket is alive — clear the pong watchdog.
    if (this._pongDeadline) { clearTimeout(this._pongDeadline); this._pongDeadline = null; }
    if (event.data instanceof ArrayBuffer) {
      const buf = new Uint8Array(event.data);
      const paneId = uuidFromBytes(buf, 0);
      const rawBytes = buf.subarray(16);
      // §perf: first PTY bytes reaching the client — record once and log all
      // three segments (each relative to connectStart) for the slow-link triage.
      if (this._perf.firstPtyBytes == null) {
        const now = performance.now();
        this._perf.firstPtyBytes = now;
        const p = this._perf;
        console.log('[remote-perf] segments', {
          upgradeMs: p.connectStart != null && p.upgradeStart != null ? Math.round(p.upgradeStart - p.connectStart) : null,
          firstFrameMs: p.connectStart != null && p.firstFrame != null ? Math.round(p.firstFrame - p.connectStart) : null,
          firstPtyBytesMs: p.connectStart != null ? Math.round(now - p.connectStart) : null,
        });
      }
      this.rawByteListeners.forEach(fn => fn(paneId, rawBytes));
      return;
    }
    try {
      const msg = JSON.parse(event.data) as WsMessage;
      if (typeof msg === 'object' && msg !== null) {
        // §data-request-fix: `data-request` replies (file tree / git / search)
        // carry NO `type` field — only `_reqId` + `_result`/`_error`. A bare
        // `(msg).type as string` then yields `undefined`, and the later
        // `type.endsWith('-result')` threw a TypeError that the outer `catch {}`
        // swallowed — so every sidebar reply was silently dropped and the
        // File/Git/Search panels never received data (一直不可用). Coalesce to ''
        // so untyped replies fall straight through to `messageListeners`, where
        // `WsDataProvider` matches them by `_reqId`.
        const type = ((msg as Record<string, unknown>).type as string) ?? '';

        // 服务端「已认证但无权」错误帧（契约形状 `{t:"error",code,message}`，用 `t`
        // 字段，非业务 `type`）。服务端升级后先发它、再以 close 4403 关闭，所以这里只
        // 暂存 code/message；随后的 _handleClose 会用它做精确分级（不重试）。
        const rec = msg as Record<string, unknown>;
        if (rec.t === 'error' && typeof rec.code === 'string') {
          this._pendingServerError = {
            code: rec.code,
            message: typeof rec.message === 'string' ? rec.message : undefined,
          };
          return;
        }

        // Heartbeat reply — liveness already recorded above, nothing else to do.
        if (type === 'pong') return;

        // New remote event types — dispatch before result routing.
        if (type === 'pty-meta') {
          const m = msg as { paneId: string; title: string | null; cwd: string | null };
          this.metaListeners.forEach(fn => fn(m.paneId, m.title, m.cwd));
          return;
        }
        if (type === 'pty-resized') {
          const r = msg as { paneId: string; rows: number; cols: number };
          this.resizeListeners.forEach(fn => fn(r.paneId, r.rows, r.cols));
          return;
        }
        if (type === 'theme') {
          const t = msg as { id?: string; themeType: 'dark' | 'light'; colors: Record<string, string> };
          // Track the active theme id so the theme-cycle button can ask the host
          // for "the theme after this one" (stateless host cycle, see cycleTheme).
          this._lastTheme = { id: t.id, themeType: t.themeType, colors: t.colors };
          this.themeListeners.forEach(fn => fn(t.colors, t.themeType));
          return;
        }

        // §history-pull: host seeds/refreshes this pane's lazy-scroll seq cursor at
        // the oldest byte currently shown (sent right after the on-subscribe tail).
        // fetchOlderScrollback() then pages older via `scrollback-before`.
        if (type === 'scrollback-meta') {
          // `scrollback-meta` isn't a WsMessage variant — read fields off the
          // untyped `rec` (like the error-frame path above) instead of casting the
          // union, which wouldn't overlap.
          this.scrollbackCursor.set(String(rec.paneId), {
            oldestSeq: Number(rec.startSeq),
            atOldest: !!rec.atOldest,
          });
          return;
        }

        // Route result-type responses to pending request promises.
        const isResult = type.endsWith('-result') || type === 'workspaces'
          || type === 'current-project' || type === 'workspace-panes';
        if (isResult) {
          const key = pendingKey(type, (msg as { _reqId?: unknown })._reqId);
          const pending = this._pendingRequests.get(key);
          if (pending) {
            this._pendingRequests.delete(key);
            pending.resolve(msg);
            return;
          }
        }
      }
      // If we're reconnecting (socket not ready), queue the message for replay.
      // Only queue non-binary messages that are state updates (not pings/pongs).
      if (this._state !== 'connected' && msg.type !== 'output') {
        this._messageQueue.push(msg);
        // If queue exceeds limit, force a full page reload to avoid stale state.
        if (this._messageQueue.length > MAX_QUEUED_MESSAGES) {
          console.warn('[wsRemote] Message queue exceeded ' + MAX_QUEUED_MESSAGES + ', reloading page');
          window.location.reload();
          return;
        }
      } else if (this._state === 'connected') {
        // Normal connected path: handle output buffering and dispatch.
        if (msg.type === 'output') {
          const lines = msg.data.split('\n');
          const existing = this.paneOutputs.get(msg.paneId) || [];
          existing.push(...lines);
          if (existing.length > MAX_PANE_OUTPUT_LINES) {
            existing.splice(0, existing.length - MAX_PANE_OUTPUT_LINES);
          }
          this.paneOutputs.set(msg.paneId, existing);
        }
        this.messageListeners.forEach(fn => fn(msg));
      }
    } catch { /* ignore */ }
  }

  // ── Drop / reconnect ──

  /**
   * onclose 入口（任务 A 问题1）：读取 close code 做失败分级。
   *   - 4403（已认证但无权）或先前已收到服务端 error 帧 → 进入 'error' 终态并按 code
   *     分级（用户问题/设备停用/通道），**不重试**——再连只会被同样拒绝、白白转圈。
   *   - 其它（正常断/网络断）→ 走原 _handleDrop 的自动重连。
   * 注：匿名/伪造在握手阶段就被不透明 403 挡住（WS 根本不 open），不会到这里。
   */
  private _handleClose(code?: number) {
    const pending = this._pendingServerError;
    if (code === WS_CLOSE_AUTHENTICATED_FORBIDDEN || pending) {
      this._pendingServerError = null;
      this._stopHeartbeat();
      if (this.ws) {
        this.ws.onopen = this.ws.onclose = this.ws.onerror = this.ws.onmessage = null;
        this.ws = null;
      }
      // 鉴权/权限类失败不重试：停掉重连定时器，标记为有意关闭，避免后台监听器重试。
      this._intentionalClose = true;
      this._clearReconnectTimer();
      const failure = classifyFailure(pending?.code, code);
      if (pending?.message) failure.message = pending.message;
      this.failWith(failure);
      return;
    }
    this._handleDrop();
  }

  private _handleDrop() {
    this._stopHeartbeat();
    if (this.ws) {
      this.ws.onopen = this.ws.onclose = this.ws.onerror = this.ws.onmessage = null;
      this.ws = null;
    }
    this._isReconnecting = true;
    this.setState('disconnected');
    if (!this._intentionalClose) this._scheduleReconnect();
  }

  private _scheduleReconnect() {
    if (this._reconnectTimer || this._intentionalClose) return;
    if (!this._host || !this._port || !this._token) return;
    const attempt = this._reconnectAttempts++;
    const base = Math.min(RECONNECT_BASE_MS * 2 ** attempt, RECONNECT_MAX_MS);
    const delay = Math.round(base + base * 0.3 * Math.random()); // jitter
    this._reconnectTimer = setTimeout(() => {
      this._reconnectTimer = null;
      if (this._intentionalClose) return;
      this._open();
    }, delay);
  }

  private _clearReconnectTimer() {
    if (this._reconnectTimer) { clearTimeout(this._reconnectTimer); this._reconnectTimer = null; }
  }

  /** Flush the queued messages after a successful reconnect. */
  private _flushQueue() {
    const queue = this._messageQueue.splice(0); // drain
    for (const msg of queue) {
      // Replay queued state messages (panes, workspaces, etc.) to listeners.
      // Skip 'output' type as it's handled via paneOutputs.
      if (msg.type !== 'output') {
        this.messageListeners.forEach(fn => fn(msg));
      }
    }
    this._isReconnecting = false;
  }

  // ── Heartbeat ──

  private _startHeartbeat() {
    this._stopHeartbeat();
    this._heartbeatTimer = setInterval(() => this._pingNow(PONG_TIMEOUT_MS), HEARTBEAT_INTERVAL_MS);
  }

  private _stopHeartbeat() {
    if (this._heartbeatTimer) { clearInterval(this._heartbeatTimer); this._heartbeatTimer = null; }
    if (this._pongDeadline) { clearTimeout(this._pongDeadline); this._pongDeadline = null; }
  }

  /** Send a ping and arm a deadline; if no inbound traffic arrives before it
   *  fires, the socket is dead (frozen/half-open) → force a drop + reconnect. */
  private _pingNow(deadlineMs: number) {
    if (this.ws?.readyState !== WebSocket.OPEN) return;
    this.send({ type: 'ping' });
    if (this._pongDeadline) clearTimeout(this._pongDeadline);
    this._pongDeadline = setTimeout(() => {
      this._pongDeadline = null;
      if (this.ws) { try { this.ws.close(); } catch { /* noop */ } }
      this._handleDrop();
    }, deadlineMs);
  }

  /** Re-probe liveness and reconnect if needed. Called on foreground/online —
   *  the moment a backgrounded mobile tab comes back is exactly when the socket
   *  is most likely silently dead. */
  ensureConnected() {
    if (this._intentionalClose) return;
    if (!this._host || !this._port || !this._token) return;
    const rs = this.ws?.readyState;
    if (rs === WebSocket.OPEN) {
      // Looks open, but a backgrounded socket can be half-dead — probe it fast.
      this._pingNow(LIVENESS_PROBE_TIMEOUT_MS);
      return;
    }
    if (rs === WebSocket.CONNECTING) return;
    // closed/closing → reconnect now and reset backoff for a snappy resume.
    this._clearReconnectTimer();
    this._reconnectAttempts = 0;
    this._open();
  }

  private _attachWindowListeners() {
    if (this._windowListenersAttached || typeof document === 'undefined') return;
    this._windowListenersAttached = true;
    this._onVisibility = () => { if (!document.hidden) this.ensureConnected(); };
    this._onOnline = () => this.ensureConnected();
    this._onForeground = () => this.ensureConnected();
    document.addEventListener('visibilitychange', this._onVisibility);
    window.addEventListener('online', this._onOnline);
    window.addEventListener('pageshow', this._onForeground);
    window.addEventListener('focus', this._onForeground);
  }

  private _detachWindowListeners() {
    if (!this._windowListenersAttached) return;
    this._windowListenersAttached = false;
    if (this._onVisibility) document.removeEventListener('visibilitychange', this._onVisibility);
    if (this._onOnline) window.removeEventListener('online', this._onOnline);
    if (this._onForeground) {
      window.removeEventListener('pageshow', this._onForeground);
      window.removeEventListener('focus', this._onForeground);
    }
    this._onVisibility = this._onOnline = this._onForeground = null;
  }

  getPaneOutput(paneId: string): string[] {
    return this.paneOutputs.get(paneId) || [];
  }

  /** Drop cached text output for panes no longer present. The UI calls this with
   *  the host's authoritative live-pane set on every `panes` update so a
   *  long-running session can't accumulate per-pane buffers for closed panes
   *  (unbounded memory growth → OOM on mobile). */
  pruneOutputs(liveIds: Set<string>) {
    for (const id of [...this.paneOutputs.keys()]) {
      if (!liveIds.has(id)) this.paneOutputs.delete(id);
    }
    for (const id of [...this.paneWorkspaces.keys()]) {
      if (!liveIds.has(id)) this.paneWorkspaces.delete(id);
    }
  }

  send(msg: Record<string, unknown>) {
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(msg));
    }
  }

  private async _sendAndWait(request: Record<string, unknown>, responseType: string, timeoutMs = 5000): Promise<unknown> {
    const key = pendingKey(responseType, request._reqId);
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this._pendingRequests.delete(key);
        reject(new Error(`WS request ${responseType} timed out`));
      }, timeoutMs);
      this._pendingRequests.set(key, {
        resolve: (v) => { clearTimeout(timer); resolve(v); },
        reject: (e) => { clearTimeout(timer); reject(e); },
      });
      this.send(request);
    });
  }

  listPanes() { this.send({ type: 'list-panes' }); }
  subscribePane(
    paneId: string,
    opts?: { resume?: boolean; sinceSeq?: number; workspaceId?: string; active?: boolean },
  ) {
    const msg: Record<string, unknown> = { type: 'subscribe-pane', paneId };
    if (opts?.workspaceId) this.paneWorkspaces.set(paneId, opts.workspaceId);
    if (opts?.resume) msg.resume = true;
    if (opts?.sinceSeq !== undefined) msg.sinceSeq = opts.sinceSeq;
    if (opts?.workspaceId) msg.workspaceId = opts.workspaceId;
    if (opts?.active !== undefined) msg.active = opts.active;
    this.send(msg);
  }

  /**
   * §history-pull（LAN 对齐 cloudRemote）: fetch the next older batch of this
   * pane's scrollback (bytes with `seq < cursor`) to PREPEND above the current
   * buffer when the viewport nears the top. Returns the raw bytes, or `null`
   * when there's nothing to load (no cursor yet, already at oldest, a fetch is
   * in flight, or the host rejected/timed out). fetchingOlder dedup + atOldest
   * stop keep it safe under rapid scroll-up. Mirrors
   * CloudRemoteConnection.fetchOlderScrollback.
   */
  async fetchOlderScrollback(paneId: string): Promise<PendingScrollbackPage | null> {
    const cursor = this.scrollbackCursor.get(paneId);
    if (!cursor || cursor.atOldest || this.fetchingOlder.has(paneId)) return null;
    this.fetchingOlder.add(paneId);
    try {
      const result = await this._sendAndWait(
        {
          type: 'scrollback-before',
          paneId,
          workspaceId: this.paneWorkspaces.get(paneId),
          beforeSeq: cursor.oldestSeq,
          maxBytes: 64 * 1024,
        },
        'scrollback-before-result',
      ) as { bytes?: string; startSeq?: number; endSeq?: number; atOldest?: boolean };
      const startSeq = Number(result.startSeq);
      const endSeq = Number(result.endSeq);
      const bytes = result.bytes ? new TextEncoder().encode(String(result.bytes)) : new Uint8Array();
      if (!(startSeq < endSeq) || endSeq !== cursor.oldestSeq || bytes.length === 0) {
        if (bytes.length === 0 && endSeq === cursor.oldestSeq && result.atOldest) {
          this.scrollbackCursor.set(paneId, { ...cursor, atOldest: true });
        }
        this.fetchingOlder.delete(paneId);
        return null;
      }
      let settled = false;
      const finish = () => {
        if (!settled) {
          settled = true;
          this.fetchingOlder.delete(paneId);
        }
      };
      return {
        bytes,
        startSeq,
        endSeq,
        atOldest: !!result.atOldest,
        commit: () => {
          if (settled || this.scrollbackCursor.get(paneId)?.oldestSeq !== endSeq) return false;
          this.scrollbackCursor.set(paneId, { oldestSeq: startSeq, atOldest: !!result.atOldest });
          finish();
          return true;
        },
        discard: finish,
      };
    } catch {
      this.fetchingOlder.delete(paneId);
      return null;
    }
  }
  listFiles(path?: string) { this.send({ type: 'list-files', path: path || '' }); }
  listGitStatus() { this.send({ type: 'list-git-status' }); }
  sendStdin(paneId: string, data: string) { this.send({ type: 'stdin', paneId, data }); }
  /** @deprecated Host-side bookkeeping only — records a fallback size but never
   *  reflows the shared PTY (no `pty-resized` broadcast), so the remote stays
   *  clipped. The automatic resize path now uses {@link claimPane} so a viewport
   *  change actually reflows the host. Kept for protocol completeness. */
  resizePane(paneId: string, rows: number, cols: number, pixelWidth?: number, pixelHeight?: number) {
    this.send({ type: 'resize', paneId, rows, cols, pixelWidth, pixelHeight });
  }
  /** Claim the shared PTY at this client's viewport size (the "lock size" /
   *  refresh button). The backend resizes the real PTY + canonical parser and
   *  broadcasts a full repaint to every viewer; the size persists until the
   *  next claim/refresh from any endpoint.
   *
   *  Each call increments a monotonic sequence counter so the backend can
   *  ignore stale requests when multiple remotes contend for the size lock. */
  refreshPane(paneId: string, rows: number, cols: number, pixelWidth: number, pixelHeight: number) {
    this._refreshSeq++;
    this.send({ type: 'refresh-pane', paneId, rows, cols, pixelWidth, pixelHeight, seq: this._refreshSeq });
  }
  /** Implicit "I just interacted / my viewport changed" size claim. Same host
   *  effect as refreshPane (resizes the real PTY + canonical parser and
   *  broadcasts a full repaint via `pty-resized`), but reserved for the
   *  automatic viewport-driven resize path so a genuine layout change reflows
   *  the host PTY — `resize` alone is host-side bookkeeping that never reflows.
   *  Shares the monotonic seq counter so the host can drop stale claims. */
  claimPane(paneId: string, rows: number, cols: number, pixelWidth: number, pixelHeight: number) {
    this._refreshSeq++;
    this.send({ type: 'claim-pane', paneId, rows, cols, pixelWidth, pixelHeight, seq: this._refreshSeq });
  }
  lastRefreshSeq(): number { return this._refreshSeq; }

  // ── Workspace operations via WS ───────────────────────────────────
  async listWorkspaces(): Promise<{ workspaces: WorkspaceInfo[] }> {
    const data = await this._sendAndWait({ type: 'list-workspaces' }, 'workspaces') as Record<string, unknown>;
    return { workspaces: (data as { workspaces: WorkspaceInfo[] }).workspaces || [] };
  }

  // P1 roster：经 `invoke-request` 走 dispatch_invoke_request 的显式白名单边界
  //（与 tauriShim 同一信封）；回包 `{type:'invoke-result', _result|_error}`。
  async getTeammateTopology(workspaceId?: string): Promise<TeammateTopology> {
    const data = (await this._sendAndWait(
      {
        type: 'invoke-request',
        cmd: 'get_teammate_topology',
        args: workspaceId ? { workspaceId } : {},
        _reqId: ++this._reqCounter,
      },
      'invoke-result',
    )) as { _result?: TeammateTopology; _error?: unknown };
    if (data._error) throw new Error(String(data._error));
    return data._result ?? { roster: [], leaderId: null, edges: [] };
  }

  // P2 阶段 1：脱敏待审批快照（同 invoke-request 白名单边界；无 action 全文）。
  async listHitlPending(): Promise<HitlPendingItem[]> {
    const data = (await this._sendAndWait(
      { type: 'invoke-request', cmd: 'list_hitl_pending', args: {}, _reqId: ++this._reqCounter },
      'invoke-result',
    )) as { _result?: HitlPendingItem[]; _error?: unknown };
    if (data._error) throw new Error(String(data._error));
    return data._result ?? [];
  }

  // P2 阶段 2：远端裁决（nonce 单次消费；仅 approve/reject）。
  async resolveHitlRemote(
    id: string,
    nonce: string,
    verdict: 'approve' | 'reject',
  ): Promise<HitlResolveOutcome> {
    const data = (await this._sendAndWait(
      {
        type: 'invoke-request',
        cmd: 'resolve_hitl_remote',
        args: { id, nonce, verdict },
        _reqId: ++this._reqCounter,
      },
      'invoke-result',
    )) as { _result?: { outcome: HitlResolveOutcome }; _error?: unknown };
    if (data._error) throw new Error(String(data._error));
    return data._result?.outcome ?? 'already-resolved';
  }

  // iter-61：把某 pane 标记/取消标记为 agent（工作区弹层终端项的「标记」按钮）。
  // 同 invoke-request 白名单边界；agentId 由调用方给（一般取 pane 标题）。
  async markPaneAgent(
    workspaceId: string,
    paneId: string,
    on: boolean,
    agentId?: string,
  ): Promise<void> {
    const data = (await this._sendAndWait(
      {
        type: 'invoke-request',
        cmd: on ? 'register_teammate_agent' : 'release_teammate_agent',
        args: on
          ? { workspaceId, paneId, agentId: agentId || 'agent' }
          : { workspaceId, paneId },
        _reqId: ++this._reqCounter,
      },
      'invoke-result',
    )) as { _error?: unknown };
    if (data._error) throw new Error(String(data._error));
  }

  async getOrchestrationHealth(): Promise<OrchestrationHealth> {
    const data = (await this._sendAndWait(
      {
        type: 'invoke-request',
        cmd: 'get_orchestration_health',
        args: {},
        _reqId: ++this._reqCounter,
      },
      'invoke-result',
    )) as { _result?: OrchestrationHealth; _error?: unknown };
    if (data._error) throw new Error(String(data._error));
    return {
      suspendedAgents: Number(data._result?.suspendedAgents ?? 0),
      pendingHitl: Number(data._result?.pendingHitl ?? 0),
    };
  }

  /** iter-63：终端类型列表 —— 与桌面同源（host `detect_available_shells`）。 */
  async listShells(): Promise<RemoteShellInfo[]> {
    const data = (await this._sendAndWait(
      {
        type: 'invoke-request',
        cmd: 'detect_available_shells',
        args: {},
        _reqId: ++this._reqCounter,
      },
      'invoke-result',
    )) as { _result?: RemoteShellInfo[]; _error?: unknown };
    if (data._error) throw new Error(String(data._error));
    return Array.isArray(data._result) ? data._result : [];
  }

  /** iter-63：原地换 shell。两步与桌面 `changePaneShell` 逐字同序：换 → 再激活 PTY。 */
  async changePaneShell(
    workspaceId: string,
    paneId: string,
    shell: RemoteShellInfo,
  ): Promise<void> {
    const change = (await this._sendAndWait(
      {
        type: 'invoke-request',
        cmd: 'change_pane_shell',
        args: { paneId, shell: shell.program, args: shell.args ?? [] },
        _reqId: ++this._reqCounter,
      },
      'invoke-result',
    )) as { _error?: unknown };
    if (change._error) throw new Error(String(change._error));
    // 重建后必须再激活一次，否则新 PTY 没有订阅者，手机端只看到一块死屏。
    // rows/cols 交给 host 用该 pane 现有几何（远端不掌握真实网格）。
    const activate = (await this._sendAndWait(
      {
        type: 'invoke-request',
        cmd: 'activate_pane_pty',
        args: { workspaceId, paneId },
        _reqId: ++this._reqCounter,
      },
      'invoke-result',
    )) as { _error?: unknown };
    if (activate._error) throw new Error(String(activate._error));
  }

  async switchWorkspace(workspaceId: string): Promise<boolean> {
    const data = await this._sendAndWait({ type: 'switch-workspace', workspaceId }, 'switch-workspace-result') as Record<string, unknown>;
    return (data as Record<string, unknown>).success === true;
  }

  async createWorkspace(name?: string): Promise<string | null> {
    const data = await this._sendAndWait({ type: 'create-workspace', name: name || '' }, 'create-workspace-result') as Record<string, unknown>;
    return (data.success && data.workspaceId) ? String(data.workspaceId) : null;
  }

  async renameWorkspace(workspaceId: string, name: string): Promise<boolean> {
    const data = (await this._sendAndWait(
      {
        type: 'invoke-request',
        cmd: 'rename_workspace',
        args: { workspaceId, name },
        _reqId: ++this._reqCounter,
      },
      'invoke-result',
    )) as { _error?: unknown };
    return !data._error;
  }

  async saveWorkspace(workspaceId: string, name: string): Promise<boolean> {
    const data = (await this._sendAndWait(
      {
        type: 'invoke-request',
        cmd: 'save_workspace_to_file',
        args: { workspaceId, name },
        _reqId: ++this._reqCounter,
      },
      'invoke-result',
    )) as { _error?: unknown };
    return !data._error;
  }

  async listSavedWorkspaceFiles(): Promise<SavedWorkspaceFile[]> {
    const data = (await this._sendAndWait(
      {
        type: 'invoke-request',
        cmd: 'list_saved_workspace_files',
        args: {},
        _reqId: ++this._reqCounter,
      },
      'invoke-result',
    )) as { _result?: unknown; _error?: unknown };
    if (data._error) throw new Error(String(data._error));
    const raw = data._result;
    if (!Array.isArray(raw)) return [];
    return raw.map((row) => {
      const r = row as Record<string, unknown>;
      return {
        name: String(r.name ?? ''),
        path: String(r.path ?? ''),
        mtimeSecs: Number(r.mtime_secs ?? r.mtimeSecs ?? 0),
      };
    }).filter((e) => e.path.length > 0);
  }

  async openWorkspaceFromFile(path: string): Promise<string | null> {
    const data = (await this._sendAndWait(
      {
        type: 'invoke-request',
        cmd: 'open_workspace_from_file',
        args: { path },
        _reqId: ++this._reqCounter,
      },
      'invoke-result',
    )) as { _result?: unknown; _error?: unknown };
    if (data._error) throw new Error(String(data._error));
    const id = data._result;
    return id != null && String(id).length > 0 ? String(id) : null;
  }

  async createPane(shell?: string): Promise<string | null> {
    const data = await this._sendAndWait({ type: 'create-pane', shell: shell || '' }, 'create-pane-result') as Record<string, unknown>;
    return (data.success && data.paneId) ? String(data.paneId) : null;
  }

  async closePane(paneId: string): Promise<boolean> {
    const data = await this._sendAndWait({ type: 'close-pane', paneId }, 'close-pane-result') as Record<string, unknown>;
    return (data as Record<string, unknown>).success === true;
  }

  async closeWorkspace(workspaceId: string): Promise<boolean> {
    const data = await this._sendAndWait({ type: 'close-workspace', workspaceId }, 'close-workspace-result') as Record<string, unknown>;
    return (data as Record<string, unknown>).success === true;
  }

  /** List the panes of an ARBITRARY workspace without switching this client's
   *  active workspace. Backs the tree's "expand a non-active workspace to peek
   *  at its terminals" (read-only on the host). */
  async listWorkspacePanes(workspaceId: string): Promise<PaneInfo[]> {
    const data = await this._sendAndWait(
      { type: 'list-workspace-panes', workspaceId },
      'workspace-panes',
    ) as { workspaceId?: string; panes?: PaneInfo[] };
    // Guard against a stale reply for a different workspace (the response type
    // is shared across workspaces, so a fast double-tap could cross wires).
    if (data.workspaceId && data.workspaceId !== workspaceId) return [];
    return data.panes || [];
  }

  async requestCurrentProject(): Promise<string> {
    const data = await this._sendAndWait({ type: 'current-project' }, 'current-project') as Record<string, unknown>;
    return (data as { path: string }).path || '';
  }
  // ───────────────────────────────────────────────────────────────────

  disconnect() {
    this._intentionalClose = true;
    this._clearReconnectTimer();
    this._stopHeartbeat();
    this._detachWindowListeners();
    this._hasConnectedOnce = false;
    // Clear any queued messages on intentional disconnect.
    this._messageQueue.length = 0;
    this._isReconnecting = false;
    if (this.ws) {
      this.ws.onopen = null;
      this.ws.onerror = null;
      this.ws.onmessage = null;
      this.ws.onclose = null;
      this.ws.close();
      this.ws = null;
    }
    this.setState('disconnected');
    this.paneOutputs.clear();
    for (const [, pending] of this._pendingRequests) {
      pending.reject(new Error('disconnected'));
    }
    this._pendingRequests.clear();
    // §history-pull: drop per-pane seq cursors + in-flight flags so a fresh
    // transport re-seeds from the host's next `scrollback-meta`.
    this.scrollbackCursor.clear();
    this.paneWorkspaces.clear();
    this.fetchingOlder.clear();
  }

  private setState(s: ConnectionState) {
    // 一旦重新连上或开始新一轮连接，清掉旧的失败分级（避免 UI 残留过期错误）。
    if (s === 'connected' || s === 'connecting') this._failure = null;
    this._state = s;
    this.stateListeners.forEach(fn => fn(s));
  }
}
