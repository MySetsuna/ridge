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
  type: 'theme';
  themeType: 'dark' | 'light';
  colors: Record<string, string>;
};

type Listener = (msg: WsMessage) => void;

export class RemoteConnection {
  private ws: WebSocket | null = null;
  private stateListeners: Set<(s: ConnectionState) => void> = new Set();
  private messageListeners: Set<Listener> = new Set();
  private binaryDeltaListeners: Set<BinaryDeltaListener> = new Set();
  private rawByteListeners: Set<RawByteListener> = new Set();
  private metaListeners: Set<MetaListener> = new Set();
  private resizeListeners: Set<PtyResizeListener> = new Set();
  private themeListeners: Set<ThemeListener> = new Set();
  private _lastTheme: { themeType: 'dark' | 'light'; colors: Record<string, string> } | null = null;
  private _state: ConnectionState = 'disconnected';
  private paneOutputs: Map<string, string[]> = new Map();
  private _pendingRequests: Map<string, { resolve: (v: unknown) => void; reject: (e: Error) => void }> = new Map();
  private _reqCounter = 0;
  private _refreshSeq = 0;
  private _host: string = '';
  private _port: number = 0;
  private _token: string = '';

  state() { return this._state; }

  onStateChange(fn: (s: ConnectionState) => void) {
    this.stateListeners.add(fn);
    return () => this.stateListeners.delete(fn);
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

  connect(host: string, port: number, auth?: string, authType: 'code' | 'token' = 'code') {
    if (this.ws) this.ws.close();
    this._host = host;
    this._port = port;
    this.setState('connecting');
    let url: string;
    if (auth) {
      const param = authType === 'token' ? 'token' : 'code';
      // Match the page's scheme: an HTTPS-served page must use wss:// (mixed
      // content blocks ws:// from https://). TLS is what unlocks WebGPU on the
      // LAN, so this is the common path in production.
      const wsScheme = location.protocol === 'https:' ? 'wss' : 'ws';
      url = `${wsScheme}://${host}:${port}/ws?${param}=${encodeURIComponent(auth)}`;
      if (authType === 'token') this._token = auth;
    } else {
      this.setState('error');
      return;
    }
    this.ws = new WebSocket(url);
    this.ws.binaryType = 'arraybuffer';

    this.ws.onopen = () => this.setState('connected');
    this.ws.onclose = () => this.setState('disconnected');
    this.ws.onerror = () => this.setState('error');

    this.ws.onmessage = (event) => {
      if (event.data instanceof ArrayBuffer) {
        const buf = new Uint8Array(event.data);
        const paneId = uuidFromBytes(buf, 0);
        const rawBytes = buf.subarray(16);
        this.rawByteListeners.forEach(fn => fn(paneId, rawBytes));
        return;
      }
      try {
        const msg = JSON.parse(event.data) as WsMessage;
        if (typeof msg === 'object' && msg !== null) {
          const type = (msg as Record<string, unknown>).type as string;

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
            const t = msg as { themeType: 'dark' | 'light'; colors: Record<string, string> };
            this._lastTheme = { themeType: t.themeType, colors: t.colors };
            this.themeListeners.forEach(fn => fn(t.colors, t.themeType));
            return;
          }

          // Route result-type responses to pending request promises.
          const isResult = type.endsWith('-result') || type === 'workspaces' || type === 'current-project';
          if (isResult) {
            const pending = this._pendingRequests.get(type);
            if (pending) {
              this._pendingRequests.delete(type);
              pending.resolve(msg);
              return;
            }
          }
        }
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
      } catch { /* ignore */ }
    };
  }

  getPaneOutput(paneId: string): string[] {
    return this.paneOutputs.get(paneId) || [];
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

  async requestCurrentProject(): Promise<string> {
    const data = await this._sendAndWait({ type: 'current-project' }, 'current-project') as Record<string, unknown>;
    return (data as { path: string }).path || '';
  }
  // ───────────────────────────────────────────────────────────────────

  disconnect() {
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
