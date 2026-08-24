import { getRemoteDeviceId } from './deviceId';
import { PaneRpcScheduler } from './paneRpcScheduler';
import { remotePerfMark } from './remotePerfTrace';
import { paneRefKey, type PaneRef, type PaneRenderOwner } from './paneRef';
import {
  tryEnqueuePaneInput,
  tryEnqueuePaneInputImmediate,
  retirePaneInput,
} from '../terminal/paneInputGate';
import {
  RpcCancelledError,
  RpcReconnectError,
  RpcTimeoutError,
  type RpcRequestOptions,
} from './types';
import { capabilityForRemoteMethod } from './capabilityContract';
import { secureRandomUnit } from './random';
import { unknownText } from './unknownText';

export type ConnectionState = 'connecting' | 'connected' | 'disconnected' | 'error';

function protocolId(value: unknown): string | null {
  return typeof value === 'string' || typeof value === 'number' ? String(value) : null;
}

function normalizeInvokeResultFrame(value: unknown): unknown {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return value;
  const frame = value as Record<string, unknown>;
  const result = frame._result;
  if (!result || typeof result !== 'object' || Array.isArray(result)) return value;
  const entries = Object.entries(result as Record<string, unknown>);
  if (entries.length !== 1) return value;
  const [key, payload] = entries[0];
  if (key === 'Ok') return { ...frame, _result: payload };
  if (key === 'Err') return { ...frame, _result: undefined, _error: payload };
  return value;
}

export { paneRefKey } from './paneRef';
export type { PaneRef, PaneRenderOwner } from './paneRef';

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

export type RawByteListener = (pane: PaneRef, data: Uint8Array) => void;
export type MetaListener = (pane: PaneRef, title: string | null, cwd: string | null) => void;
export type PtyResizeListener = (
  pane: PaneRef,
  rows: number,
  cols: number,
  owner?: PaneRenderOwner,
) => void;
export type ThemeListener = (colors: Record<string, string>, themeType: 'dark' | 'light') => void;

// Keep for backward compat — consumers should migrate to onRawBytes.

const MAX_PANE_OUTPUT_LINES = 5000;
const LAN_PANE_RPC_TIMEOUT_MS = 5_000;

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
/**
 * A first connection that never reaches `onopen` must become actionable.
 * Reconnect backoff is useful after a live session drops, but an untouched
 * remote gate must not remain in an infinite loading state.
 */
export const FIRST_CONNECT_TIMEOUT_MS = 10_000;
/** Shell discovery may enumerate WSL distributions; it is slower than a normal RPC. */
export const SHELL_DISCOVERY_TIMEOUT_MS = 30_000;

export interface PaneInfo {
  id: string;
  title?: string;
  cwd?: string;
  /** iter-61：该 pane 是否已标记为 agent（工作区弹层标记按钮的当前态）。 */
  isAgent?: boolean;
  /** Runtime teammate state, kept alongside the desktop PaneNode contract. */
  agentState?: 'idle' | 'busy' | 'starting';
  /** Stable teammate id when the host has one bound to this pane. */
  agentId?: string;
}

/**
 * 在途请求的键。**必须带 `_reqId`**（iter-63 手机端 e2e 实证）：
 * 旧实现只按 `responseType` 作键，于是任何两条并发 `invoke-request` 都注册在
 * `'invoke-result'` 这一个键上，后者 `set` 直接顶掉前者——前一条永远等不到回包，
 * 5s 后超时抛错；活下来那条还可能收到**另一条命令**的结果。手机端花名册正是
 * `Promise.all([topology, hitlPending, health])` 三连发，于是恒定失败、只显示一个「—」，
 * 而后端数据一直是对的（数据面直测 roster 完好）。
 *
 * 无 `_reqId` 的老式请求（list-workspaces 等）保持按类型作键；相同 payload 在途时
 * 共享既有 Promise，不同 payload 则串行等待（旧协议没有相关性字段，不能安全并发）。
 */
export function pendingKey(responseType: string, reqId: unknown): string {
  return typeof reqId === 'number' ? `${responseType}#${reqId}` : responseType;
}

interface PendingRequest {
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
  scope?: string;
  method?: string;
  /** Optional payload guard for legacy responses without a request id. */
  matches?: (value: unknown) => boolean;
  /** Best-effort host cancellation for a legacy invoke request. */
  cancel?: () => void;
}

interface LegacyPendingRequest {
  signature: string;
  promise: Promise<unknown>;
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
  /** Optional host snapshot fallback while the active-pane query hydrates. */
  panes?: PaneInfo[];
  /** Optional LAN host capability hint; absent means legacy full-surface host. */
  capabilities?: string[];
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
  workspaceId: string;
  paneId: string;
  data: string;
} | {
  type: 'delta';
  workspaceId: string;
  paneId: string;
  data: string;
} | {
  type: 'pty-meta';
  workspaceId: string;
  paneId: string;
  title: string | null;
  cwd: string | null;
} | {
  type: 'pty-resized';
  workspaceId: string;
  paneId: string;
  rows: number;
  cols: number;
  owner?: PaneRenderOwner;
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
  /** Live PaneHeader/OSC title; identity `name` remains stable for actions. */
  title?: string;
  /** Host-authoritative PTY CWD; UI may fall back to the live pane snapshot. */
  cwd?: string;
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
  /** Kernel-owned fencing identity; absent only for legacy hosts. */
  agentId?: string;
  sessionId?: string;
  workspaceId?: string;
  generation?: number;
  lease?: string;
  lifecycle?: string;
  online?: boolean;
  capabilities?: string[];
}

export interface AgentMessageTarget {
  workspaceId: string;
  paneId: string;
  agentId?: string;
  generation?: number;
  lease?: string;
}

export interface AgentMessageReceipt {
  messageId: string;
  deliveryId: string;
  targetKey: string;
  status: string;
  deliveryAdapter: string;
  deliveryReliability: string;
  terminalAccepted: boolean;
  agentAcknowledged: boolean;
}

/** Durable Agent history row shared by desktop and Remote.
 *
 * The host reads real Claude/Codex/Grok session files and returns this
 * projection. Keep `cwd` and `sessionId` in the wire contract: grouping by
 * Agent must not discard the context needed to resume a session.
 */
export interface AgentHistoryReply {
  agent: string;
  title: string;
  text: string;
  timestamp: number;
  cwd: string;
  sessionId: string;
  resume?: {
    executable: string;
    argv: string[];
    cwd: string;
    sessionId: string;
  };
}

/** Workspace-persisted Agent group projection. */
export interface TeammateGroup {
  id: string;
  name: string;
  color: string;
  memberAgentIds: string[];
  leaderAgentId?: string;
}

/** P1 teammate 拓扑快照（roster + leader；edges 预留）。 */
export interface TeammateTopology {
  roster: TeammateRosterMember[];
  leaderId: string | null;
  edges: unknown[];
  /** Optional for older hosts; populated by current hosts from workspace-memory. */
  groups?: TeammateGroup[];
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
  getPaneOutput(pane: PaneRef): string[];
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
    pane: PaneRef,
    opts?: {
      resume?: boolean;
      sinceSeq?: number;
      /** Foreground pane owns the transport's reserved priority lane. */
      active?: boolean;
    },
  ): void;
  /** Re-seed one pane after local render backpressure shed output. */
  resyncPane?(pane: PaneRef): void;
  /**
   * §history-pull（cloud-only）: fetch the next older batch of a pane's scrollback
   * (seq-cursor paging via get_pane_scrollback_before) to PREPEND above the current
   * buffer when the viewport nears the top. Returns the raw bytes, or null when
   * there's nothing more to load. The LAN link omits it (optional) → no-op there.
   */
  fetchOlderScrollback?(pane: PaneRef): Promise<PendingScrollbackPage | null>;
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
  sendStdin(pane: PaneRef, data: string): void;
  /** Queue a structured Hub message; PTY remains a separate compatibility path. */
  sendAgentMessage(target: AgentMessageTarget, message: string): Promise<AgentMessageReceipt>;
  /** Reserve input order before an asynchronous source resolves. */
  enqueueStdinTask?(pane: PaneRef, task: () => Promise<string | null> | string | null): boolean;
  refreshPane(
    pane: PaneRef,
    rows: number,
    cols: number,
    pixelWidth: number,
    pixelHeight: number,
    owner?: PaneRenderOwner,
  ): void;
  claimPane(
    pane: PaneRef,
    rows: number,
    cols: number,
    pixelWidth: number,
    pixelHeight: number,
    owner?: PaneRenderOwner,
  ): void;
  lastRefreshSeq(): number;
  listWorkspaces(): Promise<{ workspaces: WorkspaceInfo[] }>;
  /** P1 roster：只读拓扑快照（capability `teammate` 协商后可用；UI 轮询取数）。 */
  getTeammateTopology(workspaceId?: string): Promise<TeammateTopology>;
  /** Real session-file history, paged and searchable by the client. */
  listAgentHistory(limit?: number, offset?: number, query?: string): Promise<AgentHistoryReply[]>;
  /** Persist the current workspace's Agent groups for Remote/desktop parity. */
  setTeammateGroups(workspaceId: string, groups: readonly TeammateGroup[]): Promise<void>;
  /** Create a new pane and resume a recorded Agent session in its CWD. */
  resumeAgentSession(
    workspaceId: string,
    agent: string,
    sessionId: string,
    cwd: string,
  ): Promise<string | null>;
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
  closePane(pane: PaneRef): Promise<boolean>;
  closeWorkspace(workspaceId: string): Promise<boolean>;
  listWorkspacePanes(workspaceId: string): Promise<PaneInfo[]>;
  /** Host saved-workspace inventory (open-only on mobile; no manage). */
  listSavedWorkspaceFiles(): Promise<SavedWorkspaceFile[]>;
  /** Open a .ridge path on the host into a live workspace; returns workspace id. */
  openWorkspaceFromFile(path: string): Promise<string | null>;
  disconnect(): void;
}

/** Host saved-workspace entry (list_saved_workspace_files).
 *
 * Desktop hosts return real `~/ridge-workspaces/*.ridge` paths. The headless
 * rdg host has no `.ridge` persistence, so it returns one explicit
 * `rdg://workspace/<id>` current-workspace handle; passing that handle to
 * `openWorkspaceFromFile` is an idempotent focus operation.
 */
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
  private readonly stateListeners: Set<(s: ConnectionState) => void> = new Set();
  private readonly messageListeners: Set<Listener> = new Set();
  private readonly binaryDeltaListeners: Set<RawByteListener> = new Set();
  private readonly rawByteListeners: Set<RawByteListener> = new Set();
  private readonly metaListeners: Set<MetaListener> = new Set();
  private readonly resizeListeners: Set<PtyResizeListener> = new Set();
  private readonly themeListeners: Set<ThemeListener> = new Set();
  private _lastTheme: { id?: string; themeType: 'dark' | 'light'; colors: Record<string, string> } | null = null;
  private _state: ConnectionState = 'disconnected';
  // 最近一次失败分级（任务 A 问题1）。进入 'error' 时填充，恢复/重连时清空。
  private _failure: ConnectionFailure | null = null;
  // 服务端升级后下发的 `{t:"error",code}` 暂存：onclose(4403) 紧随其后，用它做精确分级。
  private _pendingServerError: { code?: string; message?: string } | null = null;
  private readonly paneOutputs: Map<string, string[]> = new Map();
  private readonly _pendingRequests: Map<string, PendingRequest> = new Map();
  private readonly _pendingByScope: Map<string, Set<string>> = new Map();
  /** Legacy response frames omit `_reqId`; share one in-flight request per
   * response type instead of replacing the previous pending resolver. */
  private readonly _legacyRequests: Map<string, LegacyPendingRequest> = new Map();
  /** Null preserves the legacy LAN contract until a host advertises a hint. */
  private _capabilities: Set<string> | null = null;
  /** Runtime breaker for older hosts that reject a coarse capability method. */
  private readonly _unsupportedCapabilities = new Set<string>();
  private readonly capabilityListeners: Set<() => void> = new Set();
  private _reqCounter = 0;
  private _refreshSeq = 0;
  private readonly paneScheduler = new PaneRpcScheduler(
    {
      request: <T = unknown>(method: string, params?: unknown, options?: RpcRequestOptions) =>
        this._requestPaneRpc<T>(method, params, options),
      cancelScope: (scope: string) => this._cancelPaneRpcScope(scope),
    },
    { inputSourceId: 'lan_remote' },
  );
  private _host: string = '';
  private _port: number = 0;
  private _token: string = '';
  private _authType: 'code' | 'token' = 'code';
  private _secure: boolean | null = null;

  // ── Reconnect / heartbeat state ──
  private _intentionalClose = false;
  private _reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private _reconnectAttempts = 0;
  private _initialConnectTimer: ReturnType<typeof setTimeout> | null = null;
  private _heartbeatTimer: ReturnType<typeof setInterval> | null = null;
  private _pongDeadline: ReturnType<typeof setTimeout> | null = null;
  private _hasConnectedOnce = false;
  private readonly reconnectListeners: Set<() => void> = new Set();
  private _windowListenersAttached = false;
  private _onVisibility: (() => void) | null = null;
  private _onOnline: (() => void) | null = null;
  private _onForeground: (() => void) | null = null;

  // ── Message queue for buffering during disconnect ──
  private readonly _messageQueue: WsMessage[] = [];
  private _isReconnecting = false;

  // ── §history-pull（LAN 对齐 cloudRemote）──
  // 每 pane 的 seq 游标：订阅时由 host 的 `scrollback-meta` 帧播种（首屏 tail 的最旧字节），
  // 用户滚顶时经 `scrollback-before` 分批向更旧推进。atOldest 后停止分页。
  private readonly scrollbackCursor = new Map<string, { oldestSeq: number; atOldest: boolean }>();
  /** Binary pane frames carry paneId; bind them to an explicit composite ref. */
  private readonly paneRefs = new Map<string, PaneRef>();
  /** Reverse index keeps binary PTY dispatch O(1) in the number of refs. */
  private readonly paneKeysById = new Map<string, Set<string>>();
  // 正在拉取更旧历史的 pane（去重快速连续的滚顶加载）。
  private readonly fetchingOlder = new Set<string>();

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
  hasCapability(capability: string) {
    return (this._capabilities === null || this._capabilities.has(capability))
      && !this._unsupportedCapabilities.has(capability);
  }
  onCapabilitiesChanged(fn: () => void) {
    this.capabilityListeners.add(fn);
    fn();
    return () => this.capabilityListeners.delete(fn);
  }

  private _setPaneRef(pane: PaneRef): void {
    const key = paneRefKey(pane);
    if (this.paneRefs.has(key)) {
      this.paneRefs.set(key, pane);
      return;
    }
    this.paneRefs.set(key, pane);
    let keys = this.paneKeysById.get(pane.paneId);
    if (!keys) {
      keys = new Set<string>();
      this.paneKeysById.set(pane.paneId, keys);
    }
    keys.add(key);
  }

  private _deletePaneRef(key: string): void {
    const pane = this.paneRefs.get(key);
    if (!pane) return;
    this.paneRefs.delete(key);
    const keys = this.paneKeysById.get(pane.paneId);
    if (!keys) return;
    keys.delete(key);
    if (keys.size === 0) this.paneKeysById.delete(pane.paneId);
  }

  private notifyCapabilitiesChanged() {
    for (const fn of this.capabilityListeners) {
      try { fn(); } catch { /* listener owns its errors */ }
    }
  }

  private applyAdvertisedCapabilities(capabilities: readonly string[]) {
    const next = new Set(capabilities.filter((capability): capability is string => typeof capability === 'string'));
    const previous = this._capabilities;
    const changed = previous === null
      || next.size !== previous?.size
      || [...next].some((capability) => !previous?.has(capability));
    this._capabilities = next;
    this._unsupportedCapabilities.clear();
    if (changed) this.notifyCapabilitiesChanged();
  }

  private noteUnsupportedMethod(request: Record<string, unknown>, value: unknown) {
    const method = typeof request.cmd === 'string' ? request.cmd : undefined;
    const error = (value as { _error?: unknown } | null)?._error;
    if (!method || typeof error !== 'string' || !error.includes('method not supported')) return;
    const capability = capabilityForRemoteMethod(method);
    if (!capability || this._unsupportedCapabilities.has(capability)) return;
    this._unsupportedCapabilities.add(capability);
    this.notifyCapabilitiesChanged();
  }

  /** 进入 'error' 终态并记录失败分级（供 UI 区分用户问题 / 通道异常 / 设备停用）。 */
  private failWith(failure: ConnectionFailure) {
    this._clearInitialConnectTimer();
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

  onBinaryDelta(fn: RawByteListener) {
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
    this._capabilities = null;
    this._unsupportedCapabilities.clear();
    this._clearReconnectTimer();
    this._intentionalClose = false;
    this._reconnectAttempts = 0;
    this._host = host;
    this._port = port;
    this._token = auth;
    this._authType = authType;
    this._secure = secure ?? null;
    this._clearInitialConnectTimer();
    if (!this._hasConnectedOnce) {
      this._initialConnectTimer = setTimeout(() => {
        this._initialConnectTimer = null;
        if (this._hasConnectedOnce || this._intentionalClose || this._state === 'connected') return;
        this._intentionalClose = true;
        this._clearReconnectTimer();
        this._stopHeartbeat();
        if (this.ws) {
          this.ws.onopen = this.ws.onclose = this.ws.onerror = this.ws.onmessage = null;
          try { this.ws.close(); } catch { /* already closing */ }
          this.ws = null;
        }
        this.failWith({ category: 'channel', message: 'initial remote connection timed out' });
      }, FIRST_CONNECT_TIMEOUT_MS);
    }
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
      this._clearInitialConnectTimer();
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
        queueMicrotask(() => this.paneScheduler.resumeAll());
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

  private _handleBinaryMessage(data: ArrayBuffer): void {
    const buf = new Uint8Array(data);
    const paneId = uuidFromBytes(buf, 0);
    const rawBytes = buf.subarray(16);
    remotePerfMark('raw-receive', { paneKey: paneId, bytes: rawBytes.byteLength, transport: 'lan-ws' });
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
    const keys = this.paneKeysById.get(paneId);
    if (keys?.size !== 1) return;
    const key = keys.values().next().value as string | undefined;
    const pane = key ? this.paneRefs.get(key) : undefined;
    if (pane) this.rawByteListeners.forEach((fn) => fn(pane, rawBytes));
  }

  private _handlePtyEvent(msg: WsMessage, type: string, rec: Record<string, unknown>): boolean {
    if (type === 'pty-meta') {
      if (typeof rec.workspaceId !== 'string' || typeof rec.paneId !== 'string') return true;
      this.metaListeners.forEach((fn) => fn(
        { workspaceId: rec.workspaceId as string, paneId: rec.paneId as string },
        (msg as { title: string | null }).title,
        (msg as { cwd: string | null }).cwd,
      ));
      return true;
    }
    if (type === 'pty-resized') {
      if (typeof rec.workspaceId !== 'string' || typeof rec.paneId !== 'string') return true;
      this.resizeListeners.forEach((fn) => fn(
        { workspaceId: rec.workspaceId as string, paneId: rec.paneId as string },
        (msg as { rows: number }).rows,
        (msg as { cols: number }).cols,
        (msg as { owner?: PaneRenderOwner }).owner === 'remote' ? 'remote' : 'host',
      ));
      return true;
    }
    if (type !== 'theme') return false;
    const theme = msg as { id?: string; themeType: 'dark' | 'light'; colors: Record<string, string> };
    this._lastTheme = { id: theme.id, themeType: theme.themeType, colors: theme.colors };
    this.themeListeners.forEach((fn) => fn(theme.colors, theme.themeType));
    return true;
  }

  private _handleScrollbackMeta(rec: Record<string, unknown>): void {
    if (typeof rec.workspaceId !== 'string' || typeof rec.paneId !== 'string') return;
    this.scrollbackCursor.set(paneRefKey({
      workspaceId: rec.workspaceId,
      paneId: rec.paneId,
    }), {
      oldestSeq: Number(rec.startSeq),
      atOldest: !!rec.atOldest,
    });
  }

  private _resolvePendingMessage(msg: WsMessage, type: string): boolean {
    const isResult = type.endsWith('-result') || type === 'workspaces' || type === 'current-project' || type === 'workspace-panes';
    if (!isResult) return false;
    const key = pendingKey(type, (msg as { _reqId?: unknown })._reqId);
    const pending = this._pendingRequests.get(key);
    if (!pending) return false;
    if (pending.matches && !pending.matches(msg)) return true;
    this._removePending(key);
    pending.resolve(type === 'invoke-result' ? normalizeInvokeResultFrame(msg) : msg);
    return true;
  }

  private _handlePriorityMessage(msg: WsMessage): boolean {
    const rec = msg as Record<string, unknown>;
    const type = typeof rec.type === 'string' ? rec.type : '';
    if (rec.t === 'error' && typeof rec.code === 'string') {
      this._pendingServerError = { code: rec.code, message: typeof rec.message === 'string' ? rec.message : undefined };
      return true;
    }
    if (type === 'pong') return true;
    if (type === 'hello' && Array.isArray(rec.capabilities)) this.applyAdvertisedCapabilities(rec.capabilities);
    if (this._handlePtyEvent(msg, type, rec)) return true;
    if (type === 'scrollback-meta') {
      this._handleScrollbackMeta(rec);
      return true;
    }
    return this._resolvePendingMessage(msg, type);
  }

  private _queueOrDispatchMessage(msg: WsMessage): void {
    if (this._state !== 'connected') {
      if (msg.type !== 'output') {
        this._messageQueue.push(msg);
        if (this._messageQueue.length > MAX_QUEUED_MESSAGES) {
          console.warn('[wsRemote] Message queue exceeded ' + MAX_QUEUED_MESSAGES + ', reloading page');
          window.location.reload();
        }
      }
      return;
    }
    if (msg.type !== 'output') {
      this.messageListeners.forEach((fn) => fn(msg));
      return;
    }
    if (typeof msg.workspaceId !== 'string') return;
    const key = paneRefKey({ workspaceId: msg.workspaceId, paneId: msg.paneId });
    const lines = msg.data.split('\n');
    const existing = this.paneOutputs.get(key) || [];
    existing.push(...lines);
    if (existing.length > MAX_PANE_OUTPUT_LINES) existing.splice(0, existing.length - MAX_PANE_OUTPUT_LINES);
    this.paneOutputs.set(key, existing);
    this.messageListeners.forEach((fn) => fn(msg));
  }

  private _handleMessageRefactored(event: MessageEvent): void {
    this._perf.firstFrame ??= performance.now();
    if (this._pongDeadline) { clearTimeout(this._pongDeadline); this._pongDeadline = null; }
    if (event.data instanceof ArrayBuffer) {
      this._handleBinaryMessage(event.data);
      return;
    }
		let msg: WsMessage;
		try { msg = JSON.parse(event.data) as WsMessage; }
		catch { return; }
    if (typeof msg !== 'object' || msg === null) return;
    if (this._handlePriorityMessage(msg)) return;
    this._queueOrDispatchMessage(msg);
  }

  private _handleMessage(event: MessageEvent) {
    this._handleMessageRefactored(event);
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
      this._rejectPaneRpcRequests((method) => new RpcCancelledError(method));
      this.paneScheduler.dispose();
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
    this._rejectPaneRpcRequests((method) => new RpcReconnectError(method));
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
    const delay = Math.round(base + base * 0.3 * secureRandomUnit()); // jitter
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

  getPaneOutput(pane: PaneRef): string[] {
    return this.paneOutputs.get(paneRefKey(pane)) || [];
  }

  /** Drop cached text output for panes no longer present. The UI calls this with
   *  the host's authoritative live-pane set on every `panes` update so a
   *  long-running session can't accumulate per-pane buffers for closed panes
   *  (unbounded memory growth → OOM on mobile). */
  pruneOutputs(liveIds: Set<string>) {
    const retired = this.paneScheduler.prune(liveIds);
    for (const key of retired) retirePaneInput(key);
    for (const id of this.paneOutputs.keys()) {
      if (!liveIds.has(id)) this.paneOutputs.delete(id);
    }
    for (const [key, pane] of this.paneRefs) {
      if (!liveIds.has(paneRefKey(pane))) this._deletePaneRef(key);
    }
  }

  send(msg: Record<string, unknown>) {
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(msg));
    }
  }

  private _removePending(key: string): PendingRequest | undefined {
    const pending = this._pendingRequests.get(key);
    if (!pending) return undefined;
    this._pendingRequests.delete(key);
    if (pending.scope) {
      const keys = this._pendingByScope.get(pending.scope);
      keys?.delete(key);
      if (keys?.size === 0) this._pendingByScope.delete(pending.scope);
    }
    return pending;
  }

  private _cancelPaneRpcScope(scope: string): number {
    const keys = [...(this._pendingByScope.get(scope) ?? [])];
    let cancelled = 0;
    for (const key of keys) {
      const pending = this._removePending(key);
      if (!pending) continue;
      cancelled += 1;
      pending.cancel?.();
      pending.reject(new RpcCancelledError(pending.method ?? 'pane-rpc'));
    }
    return cancelled;
  }

  /** Reject scoped pane work when the socket drops; the scheduler retries with
   * backoff, while unrelated UI snapshots retain their existing timeout path. */
  private _rejectPaneRpcRequests(errorFor: (method: string) => Error): void {
    const keys = [...this._pendingByScope.values()].flatMap((set) => [...set]);
    for (const key of keys) {
      const pending = this._removePending(key);
      if (pending) {
        pending.cancel?.();
        pending.reject(errorFor(pending.method ?? 'pane-rpc'));
      }
    }
  }

  private _requestPaneRpc<T = unknown>(
    method: string,
    params: unknown,
    options: RpcRequestOptions = {},
  ): Promise<T> {
    const scope = options.scope;
    if (!scope) return Promise.reject(new RpcCancelledError(method));
    if (!params || typeof params !== 'object' || Array.isArray(params)) {
      return Promise.reject(new TypeError(`Invalid pane RPC params: ${method}`));
    }
    if (options.signal?.aborted) return Promise.reject(new RpcCancelledError(method));
    if (this._state !== 'connected' || this.ws?.readyState !== WebSocket.OPEN) {
      return Promise.reject(new RpcReconnectError(method));
    }
    const request = {
      type: 'invoke-request',
      cmd: method,
      args: params as Record<string, unknown>,
      _reqId: ++this._reqCounter,
    };
    return this._sendAndWait(
      request,
      'invoke-result',
      options.timeoutMs ?? LAN_PANE_RPC_TIMEOUT_MS,
      { scope, method, signal: options.signal },
    ).then((value) => {
      const data = value as { _result?: unknown; _error?: unknown };
      if (data._error !== undefined && data._error !== null) {
        throw new Error(unknownText(data._error, 'remote error'));
      }
      return data._result as T;
    });
  }

  private async _sendAndWait(
    request: Record<string, unknown>,
    responseType: string,
    timeoutMs = 5000,
    options: {
      scope?: string;
      method?: string;
      signal?: AbortSignal;
      matches?: (value: unknown) => boolean;
    } = {},
  ): Promise<unknown> {
    const key = pendingKey(responseType, request._reqId);
    const legacy = typeof request._reqId !== 'number';
    if (legacy) {
      const existing = this._legacyRequests.get(key);
      if (existing) {
        const signature = JSON.stringify(request);
        // Identical old-protocol calls can safely share one response. If the
        // payload differs, serialize behind the active request: without a
        // wire correlation id, concurrent responses cannot be routed safely.
        if (existing.signature === signature) return existing.promise;
        return existing.promise.then(
          () => this._sendAndWait(request, responseType, timeoutMs, options),
          () => this._sendAndWait(request, responseType, timeoutMs, options),
        );
      }
    }
    const signature = legacy ? JSON.stringify(request) : '';
    const result = new Promise((resolve, reject) => {
      let cancelSent = false;
      const cancelHostRequest = () => {
        if (cancelSent || request.type !== 'invoke-request') return;
        const reqId = request._reqId;
        if (typeof reqId !== 'number' && typeof reqId !== 'string') return;
        cancelSent = true;
        // Legacy LAN hosts cannot consume native JSON-RPC cancellation while
        // translating invoke frames. Keep this wire-level cancellation additive
        // so old hosts ignore it and new hosts abort the owned task.
        this.send({ type: 'invoke-cancel', _reqId: reqId });
      };
      const timer = setTimeout(() => {
        const pending = this._removePending(key);
        if (!pending) return;
        pending.cancel?.();
        pending.reject(
          pending.method
            ? new RpcTimeoutError(pending.method, timeoutMs)
            : new Error(`WS request ${responseType} timed out`),
        );
      }, timeoutMs);
      const pending: PendingRequest = {
        resolve: (v) => {
          clearTimeout(timer);
          this.noteUnsupportedMethod(request, v);
          resolve(v);
        },
        reject: (e) => { clearTimeout(timer); reject(e); },
        scope: options.scope,
        method: options.method,
        matches: options.matches,
        cancel: cancelHostRequest,
      };
      this._pendingRequests.set(key, pending);
      if (options.scope) {
        let keys = this._pendingByScope.get(options.scope);
        if (!keys) {
          keys = new Set();
          this._pendingByScope.set(options.scope, keys);
        }
        keys.add(key);
      }
      const onAbort = () => {
        const current = this._removePending(key);
        if (current) {
          current.cancel?.();
          current.reject(new RpcCancelledError(options.method ?? responseType));
        }
      };
      if (options.signal) options.signal.addEventListener('abort', onAbort, { once: true });
      const resolveWithCleanup = pending.resolve;
      const rejectWithCleanup = pending.reject;
      pending.resolve = (v) => {
        if (options.signal) options.signal.removeEventListener('abort', onAbort);
        resolveWithCleanup(v);
      };
      pending.reject = (e) => {
        if (options.signal) options.signal.removeEventListener('abort', onAbort);
        rejectWithCleanup(e);
      };
      try {
        this.send(request);
      } catch (error) {
        const current = this._removePending(key);
        current?.reject(error instanceof Error ? error : new Error(String(error)));
      }
    });
    if (legacy) {
      const entry = { signature, promise: result } satisfies LegacyPendingRequest;
      this._legacyRequests.set(key, entry);
      // Remove only our own entry: a future request may have replaced it after
      // this one settled (e.g. a synchronous send failure).
      void result.then(
        () => { if (this._legacyRequests.get(key) === entry) this._legacyRequests.delete(key); },
        () => { if (this._legacyRequests.get(key) === entry) this._legacyRequests.delete(key); },
      );
    }
    return result;
  }

  listPanes() { this.send({ type: 'list-panes' }); }
  subscribePane(
    pane: PaneRef,
    opts?: { resume?: boolean; sinceSeq?: number; active?: boolean },
  ) {
    const { paneId, workspaceId } = pane;
    if (!paneId || !workspaceId) return;
    const msg: Record<string, unknown> = { type: 'subscribe-pane', paneId, workspaceId };
    this._setPaneRef(pane);
    if (opts?.resume) msg.resume = true;
    if (opts?.sinceSeq !== undefined) msg.sinceSeq = opts.sinceSeq;
    if (opts?.active !== undefined) msg.active = opts.active;
    this.send(msg);
  }

  resyncPane(pane: PaneRef) {
    this.subscribePane(pane, { active: true });
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
  async fetchOlderScrollback(pane: PaneRef): Promise<PendingScrollbackPage | null> {
    const key = paneRefKey(pane);
    const cursor = this.scrollbackCursor.get(key);
    if (!cursor || cursor.atOldest || this.fetchingOlder.has(key)) return null;
    this.fetchingOlder.add(key);
    try {
      const result = await this._sendAndWait(
        {
          type: 'scrollback-before',
          paneId: pane.paneId,
          workspaceId: pane.workspaceId,
          beforeSeq: cursor.oldestSeq,
          maxBytes: 64 * 1024,
          // The host echoes this id. Without it, simultaneous history pulls
          // for two panes would share the legacy response-type slot and one
          // pane could consume the other pane's page.
          _reqId: ++this._reqCounter,
        },
        'scrollback-before-result',
      ) as { bytes?: string; startSeq?: number; endSeq?: number; atOldest?: boolean };
      const startSeq = Number(result.startSeq);
      const endSeq = Number(result.endSeq);
      const bytes = result.bytes ? new TextEncoder().encode(String(result.bytes)) : new Uint8Array();
      if (startSeq >= endSeq || endSeq !== cursor.oldestSeq || bytes.length === 0) {
        if (bytes.length === 0 && endSeq === cursor.oldestSeq && result.atOldest) {
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
        startSeq,
        endSeq,
        atOldest: !!result.atOldest,
        commit: () => {
          if (settled || this.scrollbackCursor.get(key)?.oldestSeq !== endSeq) return false;
          this.scrollbackCursor.set(key, { oldestSeq: startSeq, atOldest: !!result.atOldest });
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
  listFiles(path?: string) { this.send({ type: 'list-files', path: path || '' }); }
  listGitStatus() { this.send({ type: 'list-git-status' }); }
  sendStdin(pane: PaneRef, data: string): boolean {
    if (!pane.paneId || !data) return false;
    const key = paneRefKey(pane);
    return tryEnqueuePaneInputImmediate(key, () => {
      this.paneScheduler.enqueueInput(pane, data);
    });
  }

  enqueueStdinTask(pane: PaneRef, task: () => Promise<string | null> | string | null): boolean {
    if (!pane.paneId) return false;
    const key = paneRefKey(pane);
    return tryEnqueuePaneInput(key, async () => {
      const data = await task();
      if (data) this.paneScheduler.enqueueInput(pane, data);
    });
  }
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
  refreshPane(
    pane: PaneRef,
    rows: number,
    cols: number,
    _pixelWidth: number,
    _pixelHeight: number,
    owner?: PaneRenderOwner,
  ) {
    const accepted = owner
      ? this.paneScheduler.scheduleResize(pane, rows, cols, { owner }, { force: true })
      : this.paneScheduler.scheduleResize(pane, rows, cols, undefined, { force: true });
    if (accepted) this._refreshSeq++;
  }
  /** Implicit "I just interacted / my viewport changed" size claim. Same host
   *  effect as refreshPane (resizes the real PTY + canonical parser and
   *  broadcasts a full repaint via `pty-resized`), but reserved for the
   *  automatic viewport-driven resize path so a genuine layout change reflows
   *  the host PTY — `resize` alone is host-side bookkeeping that never reflows.
   *  Shares the monotonic seq counter so the host can drop stale claims. */
  claimPane(
    pane: PaneRef,
    rows: number,
    cols: number,
    _pixelWidth: number,
    _pixelHeight: number,
    owner?: PaneRenderOwner,
  ) {
    this.paneScheduler.resume(pane);
    const accepted = owner
      ? this.paneScheduler.scheduleResize(pane, rows, cols, { owner }, { force: true })
      : this.paneScheduler.scheduleResize(pane, rows, cols, undefined, { force: true });
    if (accepted) this._refreshSeq++;
  }
  lastRefreshSeq(): number { return this._refreshSeq; }

  get rpcSchedulingDiagnostics() {
    return this.paneScheduler.diagnostics;
  }

  // ── Workspace operations via WS ───────────────────────────────────
  async listWorkspaces(): Promise<{ workspaces: WorkspaceInfo[] }> {
    const data = await this._sendAndWait({ type: 'list-workspaces' }, 'workspaces') as Record<string, unknown>;
    const workspaces = (data as { workspaces: WorkspaceInfo[] }).workspaces || [];
    const hinted = workspaces.find((workspace) => Array.isArray(workspace.capabilities))?.capabilities;
    if (hinted) this.applyAdvertisedCapabilities(hinted);
    return { workspaces };
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
    if (data._error) throw new Error(unknownText(data._error, 'remote error'));
    return data._result ?? { roster: [], leaderId: null, edges: [] };
  }

  async sendAgentMessage(
    target: AgentMessageTarget,
    message: string,
  ): Promise<AgentMessageReceipt> {
    const data = (await this._sendAndWait(
      {
        type: 'invoke-request',
        cmd: 'send_agent_message',
        args: {
          target_pane_id: target.paneId,
          workspace_id: target.workspaceId,
          agent_id: target.agentId,
          generation: target.generation,
          lease: target.lease,
          message,
          from: 'remote-ui',
          idempotency_key: crypto.randomUUID(),
        },
        _reqId: ++this._reqCounter,
      },
      'invoke-result',
    )) as { _result?: unknown; _error?: unknown };
    if (data._error) {
      const message = typeof data._error === 'string'
        ? data._error
        : unknownText(data._error, 'remote error');
      throw new Error(message);
    }
    if (!data._result || typeof data._result !== 'object') {
      throw new Error('Agent Hub returned an invalid receipt');
    }
    return data._result as AgentMessageReceipt;
  }

  async listAgentHistory(limit = 24, offset = 0, query = ''): Promise<AgentHistoryReply[]> {
    const data = (await this._sendAndWait(
      {
        type: 'invoke-request',
        cmd: 'read_agent_recent_replies',
        args: {
          projectPaths: [],
          limit: Math.max(1, Math.min(100, Math.floor(limit))),
          offset: Math.max(0, Math.floor(offset)),
          query: query.trim(),
        },
        _reqId: ++this._reqCounter,
      },
      'invoke-result',
    )) as { _result?: unknown; _error?: unknown };
    if (data._error) throw new Error(unknownText(data._error, 'remote error'));
    return Array.isArray(data._result) ? data._result as AgentHistoryReply[] : [];
  }

  async setTeammateGroups(
    workspaceId: string,
    groups: readonly TeammateGroup[],
  ): Promise<void> {
    const data = (await this._sendAndWait(
      {
        type: 'invoke-request',
        cmd: 'set_teammate_groups',
        args: { workspaceId, groups },
        _reqId: ++this._reqCounter,
      },
      'invoke-result',
    )) as { _error?: unknown };
    if (data._error) throw new Error(unknownText(data._error, 'remote error'));
  }

  async resumeAgentSession(
    workspaceId: string,
    agent: string,
    sessionId: string,
    cwd: string,
  ): Promise<string | null> {
    const data = (await this._sendAndWait(
      {
        type: 'invoke-request',
        cmd: 'resume_agent_session',
        args: { workspaceId, agent, sessionId, cwd },
        _reqId: ++this._reqCounter,
      },
      'invoke-result',
    )) as { _result?: { paneId?: unknown }; _error?: unknown };
    if (data._error) throw new Error(unknownText(data._error, 'remote error'));
    const paneId = data._result?.paneId;
    return typeof paneId === 'string' && paneId.length > 0 ? paneId : null;
  }

  // P2 阶段 1：脱敏待审批快照（同 invoke-request 白名单边界；无 action 全文）。
  async listHitlPending(): Promise<HitlPendingItem[]> {
    const data = (await this._sendAndWait(
      { type: 'invoke-request', cmd: 'list_hitl_pending', args: {}, _reqId: ++this._reqCounter },
      'invoke-result',
    )) as { _result?: HitlPendingItem[]; _error?: unknown };
    if (data._error) throw new Error(unknownText(data._error, 'remote error'));
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
    if (data._error) throw new Error(unknownText(data._error, 'remote error'));
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
    if (data._error) throw new Error(unknownText(data._error, 'remote error'));
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
    if (data._error) throw new Error(unknownText(data._error, 'remote error'));
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
      SHELL_DISCOVERY_TIMEOUT_MS,
    )) as { _result?: RemoteShellInfo[]; _error?: unknown };
    if (data._error) throw new Error(unknownText(data._error, 'remote error'));
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
        args: { workspaceId, paneId, shell: shell.program, args: shell.args ?? [] },
        _reqId: ++this._reqCounter,
      },
      'invoke-result',
    )) as { _error?: unknown };
    if (change._error) throw new Error(unknownText(change._error, 'remote error'));
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
    if (activate._error) throw new Error(unknownText(activate._error, 'remote error'));
  }

  async switchWorkspace(workspaceId: string): Promise<boolean> {
    const data = await this._sendAndWait({ type: 'switch-workspace', workspaceId }, 'switch-workspace-result') as Record<string, unknown>;
    return (data as Record<string, unknown>).success === true;
  }

  async createWorkspace(name?: string): Promise<string | null> {
    const data = await this._sendAndWait({ type: 'create-workspace', name: name || '' }, 'create-workspace-result') as Record<string, unknown>;
    return data.success ? protocolId(data.workspaceId) : null;
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
    if (data._error) throw new Error(unknownText(data._error, 'remote error'));
    const raw = data._result;
    if (!Array.isArray(raw)) return [];
    return raw.map((row) => {
      const r = row as Record<string, unknown>;
      return {
        name: typeof r.name === 'string' ? r.name : '',
        path: typeof r.path === 'string' ? r.path : '',
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
    if (data._error) throw new Error(unknownText(data._error, 'remote error'));
    const id = data._result;
    const idText = protocolId(id);
    return idText && idText.length > 0 ? idText : null;
  }

  async createPane(shell?: string): Promise<string | null> {
    const data = await this._sendAndWait({ type: 'create-pane', shell: shell || '' }, 'create-pane-result') as Record<string, unknown>;
    return data.success ? protocolId(data.paneId) : null;
  }

  async closePane(pane: PaneRef): Promise<boolean> {
    // Stop writes/resizes before asking the host to destroy the PTY. Otherwise
    // a late keyboard/layout event can enqueue against a pane that is already
    // being torn down, producing `Pane not found` and a retry storm.
    this.paneScheduler.retire(pane);
    const key = paneRefKey(pane);
    retirePaneInput(key);
    const data = await this._sendAndWait({
      type: 'close-pane',
      paneId: pane.paneId,
      workspaceId: pane.workspaceId,
    }, 'close-pane-result') as Record<string, unknown>;
    const closed = (data as Record<string, unknown>).success === true;
    if (closed) {
      this.paneOutputs.delete(key);
      this._deletePaneRef(key);
      this.scrollbackCursor.delete(key);
      this.fetchingOlder.delete(key);
    }
    return closed;
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
      5000,
      {
        matches: (value) => {
          const response = value as { workspaceId?: unknown };
          return response.workspaceId === undefined
            || response.workspaceId === workspaceId;
        },
      },
    ) as { workspaceId?: string; panes?: PaneInfo[] };
    // Keep a defensive invariant for transports that bypass the matcher.
    if (data.workspaceId && data.workspaceId !== workspaceId) {
      throw new Error(`stale workspace-panes response for ${data.workspaceId}`);
    }
    return data.panes || [];
  }

  async requestCurrentProject(): Promise<string> {
    const data = await this._sendAndWait({ type: 'current-project' }, 'current-project') as Record<string, unknown>;
    return (data as { path: string }).path || '';
  }
  // ───────────────────────────────────────────────────────────────────

  disconnect() {
    this._clearInitialConnectTimer();
    for (const pane of this.paneRefs.values()) retirePaneInput(paneRefKey(pane));
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
    this.paneScheduler.dispose();
    for (const [, pending] of this._pendingRequests) {
      pending.reject(new Error('disconnected'));
    }
    this._pendingRequests.clear();
    this._pendingByScope.clear();
    this._legacyRequests.clear();
    // §history-pull: drop per-pane seq cursors + in-flight flags so a fresh
    // transport re-seeds from the host's next `scrollback-meta`.
    this.scrollbackCursor.clear();
    this.paneRefs.clear();
    this.paneKeysById.clear();
    this.fetchingOlder.clear();
  }

  private setState(s: ConnectionState) {
    // 一旦重新连上或开始新一轮连接，清掉旧的失败分级（避免 UI 残留过期错误）。
    if (s === 'connected' || s === 'connecting') this._failure = null;
    this._state = s;
    this.stateListeners.forEach(fn => fn(s));
  }

  private _clearInitialConnectTimer() {
    if (this._initialConnectTimer) {
      clearTimeout(this._initialConnectTimer);
      this._initialConnectTimer = null;
    }
  }
}
