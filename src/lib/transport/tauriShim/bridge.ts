// src/lib/transport/tauriShim/bridge.ts
//
// Central bridge for the "desktop UI in a plain browser" build. The full
// desktop SvelteKit app (src/routes + src/lib) calls Tauri's `invoke()`,
// `listen()`, `Channel`, `getCurrentWindow()`, dialog and clipboard directly.
// In a browser there is no Tauri runtime, so the web-remote build aliases
// `@tauri-apps/api/*` (+ the two plugins) to the shim modules in this folder,
// and every shim routes through this singleton.
//
// Transport layering (handoff plan §5.3, D6/D7): the bridge no longer talks to
// a concrete `RemoteConnection`. It depends on
//   • the L2 shared `RpcClient` for invoke/notify/events (correlation, timeout,
//     cancel and reconnect-reject live there, written once), and
//   • the L1 `ChannelTransport.onPaneBytes` for the raw PTY byte fan-out.
// Adapters (LAN-WS today, cloud-WebRTC later) implement the L1 primitives and
// own any wire translation. The bridge is transport-agnostic.
//
// Logical wire (host = src-tauri/src/remote/server.rs, via the LAN-WS adapter):
//   • invoke  →  RpcClient.request(method, params)  (adapter maps to the host's
//                legacy invoke-request/invoke-result envelope; behavior unchanged)
//   • events  ←  host push `{type:'event', name, payload}` (notification-style),
//                surfaced to L2's notification dispatch via the adapter.
//   • PTY out ←  binary frame (paneId || raw bytes) → onPaneBytes → pty-output.

import { RpcClient } from '../remote/rpcClient';
import type { ChannelTransport, ControlFrame, Unsubscribe } from '../remote/types';

/** Tauri's event payload shape, replicated so listeners are drop-in compatible. */
export interface TauriEvent<T> {
  event: string;
  id: number;
  payload: T;
}

export type EventCallback<T> = (event: TauriEvent<T>) => void;
export type UnlistenFn = () => void;

const INVOKE_TIMEOUT_MS = 20_000;

class TauriBridge {
  private transport: ChannelTransport | null = null;
  private rpc: RpcClient | null = null;
  private listenerId = 0;
  // Exact-name event listeners, fed by host `{type:'event'}` pushes.
  private eventListeners = new Map<string, Map<number, EventCallback<unknown>>>();
  // PTY-output listeners, keyed by paneId, fed by the binary raw-byte fan-out.
  private ptyListeners = new Map<string, Map<number, EventCallback<{ data: string }>>>();
  // Panes we've subscribed to, so we can re-subscribe after a reconnect.
  private subscribedPanes = new Set<string>();
  private decoder = new TextDecoder();
  private disposers: Unsubscribe[] = [];

  /** True once a transport has been attached (after auth succeeds). */
  get ready(): boolean {
    return this.transport !== null;
  }

  /** Install an authenticated L1 transport. Called once from the web-remote
   *  boot in +layout.svelte after the handshake succeeds. The boot builds the
   *  adapter (e.g. `createLanWsTransport(conn)`) so the bridge stays free of any
   *  concrete-transport dependency. */
  attach(transport: ChannelTransport): void {
    this.transport = transport;
    this.rpc = new RpcClient(transport, { defaultTimeoutMs: INVOKE_TIMEOUT_MS });

    // Raw PTY bytes ride the L1 binary fan-out → pty-output-* listeners.
    this.disposers.push(transport.onPaneBytes((paneId, bytes) => this.dispatchRawBytes(paneId, bytes)));

    // Host event pushes arrive as `{type:'event', name, payload}` control frames
    // (legacy notification shape, passed through by the adapter). Tap the raw
    // control stream so we keep delivering them to exact-name listeners.
    this.disposers.push(
      transport.onControl((frame) => this.handleControlFrame(frame)),
    );

    // Re-subscribe panes after a reconnect (raw-byte snapshot re-pull, D10).
    this.disposers.push(
      this.rpc.onReconnected(() => {
        for (const paneId of this.subscribedPanes) {
          this.rpc?.notify('subscribe-pane', { paneId });
        }
      }),
    );

    // Opt into global workspace semantics: the desktop UI in a browser is a peer
    // desktop and switches workspaces via the real `switch_workspace` command, so
    // the host must track the GLOBAL active workspace for this client (not the
    // mobile per-client view). See `use_global_ws` in remote/server.rs.
    this.rpc.notify('use-global-workspace');
  }

  // ── invoke ────────────────────────────────────────────────────────────────
  invoke<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
    const rpc = this.rpc;
    if (!rpc) return Promise.reject(new Error(`invoke('${cmd}') before bridge connected`));
    return rpc.request<T>(cmd, args);
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
    this.subscribedPanes.add(paneId);
    this.rpc?.notify('subscribe-pane', { paneId });
  }

  // ── internal dispatch ─────────────────────────────────────────────────────
  /** Host pushes events as `{type:'event', name, payload}` control frames. JSON-RPC
   *  responses are consumed by the RpcClient before this runs, so we only act on
   *  the event push shape. */
  private handleControlFrame(frame: ControlFrame): void {
    if (frame.type === 'event' && typeof frame.name === 'string') {
      this.emitLocal(frame.name, frame.payload);
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
