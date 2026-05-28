import { writable, type Writable } from 'svelte/store';

export type ConnectionState = 'disconnected' | 'connecting' | 'connected' | 'error';

export interface PaneInfo {
  id: string;
  title?: string;
}

export interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
  is_ignored?: boolean | null;
  child_count?: number;
}

export interface GitStatusData {
  staged: string[];
  unstaged: { name: string; status: string }[];
  commits: { msg: string; hash: string; time: string }[];
}

export interface RemoteClientEntry {
  id: number;
  connectedAt: number;
  remoteAddr: string;
  userAgent: string;
}

export interface RemoteConnectionApi {
  state: Writable<ConnectionState>;
  error: Writable<string | null>;
  panes: Writable<PaneInfo[]>;
  currentProject: Writable<string>;
  fileEntries: Writable<FileEntry[]>;
  gitStatus: Writable<GitStatusData>;
  remoteClients: Writable<RemoteClientEntry[]>;
  connect: (host: string, port: number, code: string) => void;
  ping: () => void;
  listPanes: () => void;
  requestCurrentProject: () => void;
  listFiles: (path?: string) => void;
  listGitStatus: () => void;
  listRemoteClients: () => void;
  kickRemoteClient: (id: number) => void;
  disconnect: () => void;
  send: (msg: Record<string, unknown>) => void;
}

export function createRemoteConnection(): RemoteConnectionApi {
  const state = writable<ConnectionState>('disconnected');
  const error = writable<string | null>(null);
  const panes = writable<PaneInfo[]>([]);
  const currentProject = writable<string>('');
  const fileEntries = writable<FileEntry[]>([]);
  const gitStatus = writable<GitStatusData>({ staged: [], unstaged: [], commits: [] });
  const remoteClients = writable<RemoteClientEntry[]>([]);

  let ws: WebSocket | null = null;
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  let host = '';
  let port = 0;
  let code = '';

  function connect(h: string, p: number, c: string) {
    host = h;
    port = p;
    code = c;
    doConnect();
  }

  let intentionalDisconnect = false;

  function doConnect() {
    if (ws) {
      ws.onclose = null;
      ws.onerror = null;
      ws.close();
    }
    state.set('connecting');
    error.set(null);
    intentionalDisconnect = false;

    const url = `ws://${host}:${port}/ws?code=${encodeURIComponent(code)}`;
    ws = new WebSocket(url);

    ws.onopen = () => {
      state.set('connected');
    };

    ws.onmessage = (event) => {
      try {
        const msg = JSON.parse(event.data);
        switch (msg.type) {
          case 'panes':
            panes.set(msg.panes ?? []);
            break;
          case 'current-project':
            currentProject.set(msg.path ?? '');
            break;
          case 'files':
            fileEntries.set(msg.entries ?? []);
            break;
          case 'git-status':
            gitStatus.set({ staged: msg.staged ?? [], unstaged: msg.unstaged ?? [], commits: msg.commits ?? [] });
            break;
          case 'remote-clients':
            remoteClients.set(msg.clients ?? []);
            break;
          case 'error':
            error.set(msg.message ?? 'unknown error');
            break;
        }
      } catch { /* ignore */ }
    };

    ws.onclose = () => {
      if (!intentionalDisconnect) {
        state.set('disconnected');
        ws = null;
        scheduleReconnect();
      } else {
        state.set('disconnected');
        ws = null;
      }
    };

    ws.onerror = () => {
      error.set('WebSocket connection failed');
      ws?.close();
    };
  }

  function scheduleReconnect() {
    if (reconnectTimer) return;
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      if (host && port && code) {
        doConnect();
      }
    }, 5000);
  }

  function send(msg: Record<string, unknown>) {
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify(msg));
    }
  }

  function ping() { send({ type: 'ping' }); }

  function listPanes() { send({ type: 'list-panes' }); }
  function requestCurrentProject() { send({ type: 'current-project' }); }
  function listFiles(path?: string) { send({ type: 'list-files', path: path || '' }); }
  function listGitStatus() { send({ type: 'list-git-status' }); }
  function listRemoteClients() { send({ type: 'list-remote-clients' }); }
  function kickRemoteClient(id: number) { send({ type: 'kick-remote-client', id }); }

  function disconnect() {
    intentionalDisconnect = true;
    if (reconnectTimer) {
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
    if (ws) {
      ws.onclose = null;
      ws.onerror = null;
      ws.close();
      ws = null;
    }
    state.set('disconnected');
    host = '';
    port = 0;
    code = '';
  }

  return { state, error, panes, currentProject, fileEntries, gitStatus, remoteClients, connect, ping, listPanes, requestCurrentProject, listFiles, listGitStatus, listRemoteClients, kickRemoteClient, disconnect, send };
}
