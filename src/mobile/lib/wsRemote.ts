import type { SidebarProvider, DirListing, GitInfo, SearchHit } from '@shared/sidebar/types';

export type ConnectionState = 'connecting' | 'connected' | 'disconnected' | 'error';

/** Stable, per-device id persisted in localStorage. Sent on connect so the
 *  desktop can label sessions and blacklist a specific device (survives token
 *  rotation). `crypto.randomUUID` needs a secure context (absent on plain-http
 *  LAN), so fall back to a random string. */
export function getDeviceId(): string {
  try {
    let id = localStorage.getItem('wind-remote-device-id');
    if (!id) {
      id = (typeof crypto !== 'undefined' && crypto.randomUUID)
        ? crypto.randomUUID()
        : 'dev-' + Math.random().toString(36).slice(2) + Math.random().toString(36).slice(2);
      localStorage.setItem('wind-remote-device-id', id);
    }
    return id;
  } catch {
    return '';
  }
}

function uuidFromBytes(bytes: Uint8Array, offset: number = 0): string {
  const hex: string[] = [];
  for (let i = offset; i < offset + 16; i++) {
    hex.push(bytes[i].toString(16).padStart(2, '0'));
  }
  const h = hex.join('');
  return `${h.slice(0,8)}-${h.slice(8,12)}-${h.slice(12,16)}-${h.slice(16,20)}-${h.slice(20)}`;
}

export type BinaryDeltaListener = (paneId: string, data: Uint8Array) => void;

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
  data: string;  // base64-encoded postcard delta frame
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
  type: 'close-workspace-result';
  success: boolean;
  error?: string;
} | {
  type: 'create-pane-result';
  success: boolean;
  paneId?: string;
  error?: string;
} | {
  type: 'close-pane-result';
  success: boolean;
  error?: string;
};

type Listener = (msg: WsMessage) => void;

export class RemoteConnection implements SidebarProvider {
  private ws: WebSocket | null = null;
  private stateListeners: Set<(s: ConnectionState) => void> = new Set();
  private messageListeners: Set<Listener> = new Set();
  private binaryDeltaListeners: Set<BinaryDeltaListener> = new Set();
  private _state: ConnectionState = 'disconnected';
  private paneOutputs: Map<string, string[]> = new Map();
  private _pendingRequests: Map<string, { resolve: (v: unknown) => void; reject: (e: Error) => void }> = new Map();
  private _reqCounter = 0;
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

  connect(host: string, port: number, auth?: string, authType: 'code' | 'token' = 'code') {
    if (this.ws) this.ws.close();
    this._host = host;
    this._port = port;
    this.setState('connecting');
    let url: string;
    if (auth) {
      const param = authType === 'token' ? 'token' : 'code';
      url = `ws://${host}:${port}/ws?${param}=${encodeURIComponent(auth)}&device=${encodeURIComponent(getDeviceId())}`;
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
        const deltaBytes = buf.slice(16);
        this.binaryDeltaListeners.forEach(fn => fn(paneId, deltaBytes));
        return;
      }
      try {
        const msg = JSON.parse(event.data) as WsMessage;
        // Route result-type responses to pending request promises.
        if (typeof msg === 'object' && msg !== null) {
          const type = (msg as Record<string, unknown>).type as string;
          const isResult = type.endsWith('-result') || type === 'workspaces'
            || type === 'current-project' || type === 'files' || type === 'git-status'
            || type === 'search-results';
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
  /** §multi-size: explicitly re-claim the shared PTY at this device's size + full repaint. */
  refreshPane(paneId: string, rows: number, cols: number, pixelWidth?: number, pixelHeight?: number) {
    this.send({ type: 'refresh-pane', paneId, rows, cols, pixelWidth, pixelHeight });
  }
  /** §own-active: this client just became the active owner (genuine interaction)
   *  → resize the shared PTY to this device's size. Last interaction wins. */
  claimPane(paneId: string, rows: number, cols: number, pixelWidth?: number, pixelHeight?: number) {
    this.send({ type: 'claim-pane', paneId, rows, cols, pixelWidth, pixelHeight });
  }

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

  async closeWorkspace(workspaceId: string): Promise<boolean> {
    const data = await this._sendAndWait({ type: 'close-workspace', workspaceId }, 'close-workspace-result') as Record<string, unknown>;
    return (data as Record<string, unknown>).success === true;
  }

  // ── Terminal (pane) operations via WS — §6 ─────────────────────────
  /** Create a terminal in the current workspace (server picks a balanced split). */
  async createPane(shell?: string): Promise<string | null> {
    const data = await this._sendAndWait({ type: 'create-pane', shell: shell || '' }, 'create-pane-result') as Record<string, unknown>;
    return (data.success && data.paneId) ? String(data.paneId) : null;
  }

  async closePane(paneId: string): Promise<boolean> {
    const data = await this._sendAndWait({ type: 'close-pane', paneId }, 'close-pane-result') as Record<string, unknown>;
    return (data as Record<string, unknown>).success === true;
  }

  async requestCurrentProject(): Promise<string> {
    const data = await this._sendAndWait({ type: 'current-project' }, 'current-project') as Record<string, unknown>;
    return (data as { path: string }).path || '';
  }
  // ───────────────────────────────────────────────────────────────────

  // ── SidebarProvider: shared file/git/search components data source ──
  async listDir(path: string): Promise<DirListing> {
    const data = await this._sendAndWait({ type: 'list-files', path: path || '' }, 'files', 8000) as Record<string, unknown>;
    return {
      path: String(data.path ?? ''),
      parent: (data.parent as string | null) ?? null,
      entries: (data.entries as DirListing['entries']) ?? [],
    };
  }

  async gitStatus(): Promise<GitInfo> {
    const data = await this._sendAndWait({ type: 'list-git-status' }, 'git-status', 12000) as Record<string, unknown>;
    return {
      isGitRepo: data.isGitRepo === true,
      currentBranch: (data.currentBranch as string | null) ?? null,
      branches: (data.branches as string[]) ?? [],
      files: (data.files as GitInfo['files']) ?? [],
      commits: (data.commits as GitInfo['commits']) ?? [],
    };
  }

  async search(query: string): Promise<SearchHit[]> {
    const data = await this._sendAndWait({ type: 'search-files', query }, 'search-results', 20000) as Record<string, unknown>;
    return (data.results as SearchHit[]) ?? [];
  }
  // ───────────────────────────────────────────────────────────────────

  disconnect() {
    if (this.ws) { this.ws.onclose = null; this.ws.close(); this.ws = null; }
    this.setState('disconnected');
    this.paneOutputs.clear();
  }

  private setState(s: ConnectionState) {
    this._state = s;
    this.stateListeners.forEach(fn => fn(s));
  }
}
