export type ConnectionState = 'connecting' | 'connected' | 'disconnected' | 'error';

export interface PaneInfo {
  id: string;
  title?: string;
  cwd?: string;
}

export interface FileEntry {
  name: string;
  type: 'file' | 'dir';
  path: string;
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
};

type Listener = (msg: WsMessage) => void;

export class RemoteConnection {
  private ws: WebSocket | null = null;
  private stateListeners: Set<(s: ConnectionState) => void> = new Set();
  private messageListeners: Set<Listener> = new Set();
  private _state: ConnectionState = 'disconnected';
  private paneOutputs: Map<string, string[]> = new Map();

  get state() { return this._state; }

  onStateChange(fn: (s: ConnectionState) => void) {
    this.stateListeners.add(fn);
    return () => this.stateListeners.delete(fn);
  }

  onMessage(fn: Listener) {
    this.messageListeners.add(fn);
    return () => this.messageListeners.delete(fn);
  }

  connect(host: string, port: number, code: string) {
    if (this.ws) this.ws.close();
    this.setState('connecting');
    const url = `ws://${host}:${port}/ws?code=${encodeURIComponent(code)}`;
    this.ws = new WebSocket(url);

    this.ws.onopen = () => this.setState('connected');
    this.ws.onclose = () => this.setState('disconnected');
    this.ws.onerror = () => this.setState('error');

    this.ws.onmessage = (event) => {
      try {
        const msg = JSON.parse(event.data) as WsMessage;
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

  listPanes() { this.send({ type: 'list-panes' }); }
  listFiles(path?: string) { this.send({ type: 'list-files', path: path || '' }); }
  listGitStatus() { this.send({ type: 'list-git-status' }); }
  sendStdin(paneId: string, data: string) { this.send({ type: 'stdin', paneId, data }); }
  resizePane(paneId: string, rows: number, cols: number) { this.send({ type: 'resize', paneId, rows, cols }); }

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
