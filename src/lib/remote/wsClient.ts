import { writable, type Writable } from 'svelte/store';

export type ConnectionState = 'disconnected' | 'connecting' | 'connected' | 'error';

export interface PaneInfo {
  id: string;
  title?: string;
}

export interface RemoteConnectionApi {
  state: Writable<ConnectionState>;
  error: Writable<string | null>;
  panes: Writable<PaneInfo[]>;
  connect: (host: string, port: number, code: string) => void;
  ping: () => void;
  listPanes: () => void;
  disconnect: () => void;
  send: (msg: Record<string, unknown>) => void;
}

export function createRemoteConnection(): RemoteConnectionApi {
  const state = writable<ConnectionState>('disconnected');
  const error = writable<string | null>(null);
  const panes = writable<PaneInfo[]>([]);

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

  function doConnect() {
    if (ws) {
      ws.onclose = null;
      ws.close();
    }
    state.set('connecting');
    error.set(null);

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
          case 'error':
            error.set(msg.message ?? 'unknown error');
            break;
        }
      } catch {
        // ignore unparseable messages
      }
    };

    ws.onclose = () => {
      state.set('disconnected');
      ws = null;
      scheduleReconnect();
    };

    ws.onerror = () => {
      state.set('error');
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

  function disconnect() {
    if (reconnectTimer) {
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
    if (ws) {
      ws.onclose = null;
      ws.close();
      ws = null;
    }
    state.set('disconnected');
    host = '';
    port = 0;
    code = '';
  }

  return { state, error, panes, connect, ping, listPanes, disconnect, send };
}
