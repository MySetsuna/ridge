// src/lib/transport/tauriShim/bridge.ts
//
// Central bridge for the "desktop UI in a plain browser" build. The full
// desktop SvelteKit app (src/routes + src/lib) calls Tauri's `invoke()`,
// `listen()`, `Channel`, `getCurrentWindow()`, dialog and clipboard directly.
// In a browser there is no Tauri runtime, so the web-remote build aliases
// `@tauri-apps/api/*` (+ the two plugins) to the shim modules in this folder,
// and every shim routes through this singleton over the SAME LAN remote
// WebSocket the mobile SPA already uses (`RemoteConnection`).
//
// Wire protocol (host = src-tauri/src/remote/server.rs):
//   • invoke  →  { type:'invoke-request', cmd, args, _reqId }
//                reply { type:'invoke-result', _reqId, _result } | { ..., _error }
//   • events  ←  { type:'event', name, payload }   (host push)
//   • PTY out ←  binary frame: paneId(16 bytes) || raw bytes (existing fan-out)
//
// The reply/event frames carry a `type` field on purpose: RemoteConnection's
// onmessage does `type.endsWith('-result')` unconditionally, which throws (and
// silently drops the frame) when `type` is undefined. Stamping a type keeps the
// frame flowing to `onMessage` listeners where we correlate by `_reqId`.

import { RemoteConnection } from '../../../remote/lib/wsRemote';

/** Tauri's event payload shape, replicated so listeners are drop-in compatible. */
export interface TauriEvent<T> {
  event: string;
  id: number;
  payload: T;
}

export type EventCallback<T> = (event: TauriEvent<T>) => void;
export type UnlistenFn = () => void;

interface Pending {
  resolve: (v: unknown) => void;
  reject: (e: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

const INVOKE_TIMEOUT_MS = 20_000;

class TauriBridge {
  private conn: RemoteConnection | null = null;
  private reqId = 0;
  private listenerId = 0;
  private pending = new Map<number, Pending>();
  // Exact-name event listeners, fed by host `{type:'event'}` pushes.
  private eventListeners = new Map<string, Map<number, EventCallback<unknown>>>();
  // PTY-output listeners, keyed by paneId, fed by the binary raw-byte fan-out.
  private ptyListeners = new Map<string, Map<number, EventCallback<{ data: string }>>>();
  private decoder = new TextDecoder();

  /** True once a connection has been attached (after auth succeeds). */
  get ready(): boolean {
    return this.conn !== null;
  }

  connection(): RemoteConnection {
    if (!this.conn) throw new Error('Tauri bridge not connected');
    return this.conn;
  }

  /** Install the authenticated RemoteConnection. Called once from the
   *  web-remote boot in +layout.svelte after the TOTP handshake succeeds. */
  attach(conn: RemoteConnection): void {
    this.conn = conn;
    conn.onMessage((msg) => this.handleMessage(msg as Record<string, unknown>));
    conn.onRawBytes((paneId, bytes) => this.dispatchRawBytes(paneId, bytes));
    // Opt into global workspace semantics: the desktop UI in a browser is a peer
    // desktop and switches workspaces via the real `switch_workspace` command, so
    // the host must track the GLOBAL active workspace for this client (not the
    // mobile per-client view). See `use_global_ws` in remote/server.rs.
    conn.send({ type: 'use-global-workspace' });
  }

  // ── invoke ────────────────────────────────────────────────────────────────
  invoke<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
    const conn = this.conn;
    if (!conn) return Promise.reject(new Error(`invoke('${cmd}') before bridge connected`));
    const id = ++this.reqId;
    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`invoke('${cmd}') timed out`));
      }, INVOKE_TIMEOUT_MS);
      this.pending.set(id, { resolve: (v) => resolve(v as T), reject, timer });
      conn.send({ type: 'invoke-request', cmd, args, _reqId: id });
    });
  }

  // ── events ──────────────────────────────────────────────────────────────
  /** Subscribe to a host-pushed event. `pty-output-{ws}-{pane}` is special-
   *  cased onto the binary raw-byte stream; everything else rides the generic
   *  `{type:'event'}` push channel. */
  listen<T>(name: string, cb: EventCallback<T>): UnlistenFn {
    const id = ++this.listenerId;
    const paneId = parsePtyOutputPane(name);
    if (paneId) {
      let m = this.ptyListeners.get(paneId);
      if (!m) {
        m = new Map();
        this.ptyListeners.set(paneId, m);
      }
      m.set(id, cb as EventCallback<{ data: string }>);
      return () => this.ptyListeners.get(paneId)?.delete(id);
    }
    let m = this.eventListeners.get(name);
    if (!m) {
      m = new Map();
      this.eventListeners.set(name, m);
    }
    m.set(id, cb as EventCallback<unknown>);
    return () => this.eventListeners.get(name)?.delete(id);
  }

  /** Start the host streaming raw PTY bytes for a pane (replaces the desktop's
   *  Tauri `Channel` + `register_pane_delta_channel` path). */
  subscribePane(paneId: string): void {
    this.conn?.subscribePane(paneId);
  }

  // ── internal dispatch ─────────────────────────────────────────────────────
  private handleMessage(msg: Record<string, unknown>): void {
    const type = msg.type;
    if (type === 'invoke-result' && typeof msg._reqId === 'number') {
      const req = this.pending.get(msg._reqId);
      if (!req) return;
      clearTimeout(req.timer);
      this.pending.delete(msg._reqId);
      if (msg._error !== undefined && msg._error !== null) {
        req.reject(new Error(String(msg._error)));
      } else {
        req.resolve(msg._result ?? null);
      }
      return;
    }
    if (type === 'event' && typeof msg.name === 'string') {
      this.emitLocal(msg.name, msg.payload);
    }
  }

  private emitLocal(name: string, payload: unknown): void {
    const m = this.eventListeners.get(name);
    if (!m || m.size === 0) return;
    const evt: TauriEvent<unknown> = { event: name, id: 0, payload };
    for (const cb of m.values()) {
      try {
        cb(evt);
      } catch (e) {
        console.error(`[tauriShim] listener for "${name}" threw`, e);
      }
    }
  }

  private dispatchRawBytes(paneId: string, bytes: Uint8Array): void {
    const m = this.ptyListeners.get(paneId);
    if (!m || m.size === 0) return;
    const data = this.decoder.decode(bytes, { stream: true });
    const evt: TauriEvent<{ data: string }> = {
      event: `pty-output-${paneId}`,
      id: 0,
      payload: { data },
    };
    for (const cb of m.values()) {
      try {
        cb(evt);
      } catch (e) {
        console.error('[tauriShim] pty-output listener threw', e);
      }
    }
  }
}

/** Extract the trailing pane UUID from a `pty-output-{ws}-{pane}` event name. */
function parsePtyOutputPane(name: string): string | null {
  if (!name.startsWith('pty-output-')) return null;
  const tail = name.slice('pty-output-'.length);
  // tail = "{workspaceUuid}-{paneUuid}"; a UUID is 36 chars → pane is the last 36.
  return tail.length >= 36 ? tail.slice(tail.length - 36) : tail;
}

/** Process-wide singleton — the shims and the boot code share this instance. */
export const bridge = new TauriBridge();
