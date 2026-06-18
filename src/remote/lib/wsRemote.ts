import { getRemoteDeviceId } from './deviceId';

export type ConnectionState = 'connecting' | 'connected' | 'disconnected' | 'error';

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
export interface RemoteLink {
  state(): ConnectionState;
  onStateChange(fn: (s: ConnectionState) => void): () => void;
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
  connect(host: string, port: number, auth?: string, authType?: 'code' | 'token'): void;
  getPaneOutput(paneId: string): string[];
  pruneOutputs(liveIds: Set<string>): void;
  send(msg: Record<string, unknown>): void;
  listPanes(): void;
  subscribePane(paneId: string): void;
  sendStdin(paneId: string, data: string): void;
  refreshPane(paneId: string, rows: number, cols: number, pixelWidth: number, pixelHeight: number): void;
  claimPane(paneId: string, rows: number, cols: number, pixelWidth: number, pixelHeight: number): void;
  lastRefreshSeq(): number;
  listWorkspaces(): Promise<{ workspaces: WorkspaceInfo[] }>;
  switchWorkspace(workspaceId: string): Promise<boolean>;
  createWorkspace(name?: string): Promise<string | null>;
  createPane(shell?: string): Promise<string | null>;
  closePane(paneId: string): Promise<boolean>;
  closeWorkspace(workspaceId: string): Promise<boolean>;
  listWorkspacePanes(workspaceId: string): Promise<PaneInfo[]>;
  disconnect(): void;
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
  private paneOutputs: Map<string, string[]> = new Map();
  private _pendingRequests: Map<string, { resolve: (v: unknown) => void; reject: (e: Error) => void }> = new Map();
  private _reqCounter = 0;
  private _refreshSeq = 0;
  private _host: string = '';
  private _port: number = 0;
  private _token: string = '';
  private _authType: 'code' | 'token' = 'code';

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

  connect(host: string, port: number, auth?: string, authType: 'code' | 'token' = 'code') {
    if (!auth) { this.setState('error'); return; }
    this._clearReconnectTimer();
    this._intentionalClose = false;
    this._reconnectAttempts = 0;
    this._host = host;
    this._port = port;
    this._token = auth;
    this._authType = authType;
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
    const wsScheme = location.protocol === 'https:' ? 'wss' : 'ws';
    const param = this._authType === 'token' ? 'token' : 'code';
    // §L-3: pin the session to this device (in addition to its source IP) so a
    // token replayed from another device behind the same NAT egress can't
    // connect. MUST match the `device` sent to /verify at issuance.
    const device = encodeURIComponent(getRemoteDeviceId());
    const url = `${wsScheme}://${this._host}:${this._port}/ws?${param}=${encodeURIComponent(this._token)}&device=${device}`;
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
    ws.onclose = () => this._handleDrop();
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

        // Route result-type responses to pending request promises.
        const isResult = type.endsWith('-result') || type === 'workspaces'
          || type === 'current-project' || type === 'workspace-panes';
        if (isResult) {
          const pending = this._pendingRequests.get(type);
          if (pending) {
            this._pendingRequests.delete(type);
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
  }

  send(msg: Record<string, unknown>) {
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(msg));
    }
  }

  private async _sendAndWait(request: Record<string, unknown>, responseType: string, timeoutMs = 5000): Promise<unknown> {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this._pendingRequests.delete(responseType);
        reject(new Error(`WS request ${responseType} timed out`));
      }, timeoutMs);
      this._pendingRequests.set(responseType, {
        resolve: (v) => { clearTimeout(timer); resolve(v); },
        reject: (e) => { clearTimeout(timer); reject(e); },
      });
      this.send(request);
    });
  }

  listPanes() { this.send({ type: 'list-panes' }); }
  subscribePane(paneId: string) { this.send({ type: 'subscribe-pane', paneId }); }
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

  async switchWorkspace(workspaceId: string): Promise<boolean> {
    const data = await this._sendAndWait({ type: 'switch-workspace', workspaceId }, 'switch-workspace-result') as Record<string, unknown>;
    return (data as Record<string, unknown>).success === true;
  }

  async createWorkspace(name?: string): Promise<string | null> {
    const data = await this._sendAndWait({ type: 'create-workspace', name: name || '' }, 'create-workspace-result') as Record<string, unknown>;
    return (data.success && data.workspaceId) ? String(data.workspaceId) : null;
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
  }

  private setState(s: ConnectionState) {
    this._state = s;
    this.stateListeners.forEach(fn => fn(s));
  }
}
