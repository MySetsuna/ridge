// src/lib/transport/remote/lanWsAdapter.ts
//
// L1 ChannelTransport adapter for the LAN WebSocket leg. Wraps the existing,
// production-proven `RemoteConnection` (src/remote/lib/wsRemote.ts) so the
// desktop SPA's bridge can ride it through the L2 RpcClient — with ZERO change
// to the LAN wire protocol the host (src-tauri/src/remote/server.rs) speaks.
//
// Translation boundary (this is the whole point of keeping it in L1):
//   • L2 emits JSON-RPC 2.0 envelopes. The current LAN host predates JSON-RPC
//     and speaks the legacy `{type:'invoke-request', cmd, args, _reqId}` /
//     `{type:'invoke-result', _reqId, _result|_error}` envelope plus control
//     messages like `use-global-workspace`. So this adapter:
//       outbound:  JSON-RPC request `{jsonrpc,id,method,params}`
//                    → legacy `{type:'invoke-request', cmd:method, args:params, _reqId:id}`
//                  JSON-RPC notification `{jsonrpc,method,params}` (no id)
//                    → legacy control frame `{type:method, ...params}`
//                  `$/cancel` → legacy `{type:'cancel', _reqId:<target>}` (host
//                    may ignore it today; harmless and forward-compatible)
//       inbound:   legacy `{type:'invoke-result', _reqId, _result|_error}`
//                    → JSON-RPC response `{jsonrpc,id,result|error}`
//                  any other text frame → passed through verbatim as a control
//                    frame (callers that want raw host pushes still see them).
//   • PTY bytes ride RemoteConnection.onRawBytes untouched → onPaneBytes.
//
// When S3 lands a JSON-RPC-native LAN host, this translation collapses to a
// pass-through; L2 and the bridge never change. Behavior on the wire today is
// byte-for-byte identical to the pre-refactor bridge.

import { RemoteConnection, type ConnectionState } from '../../../remote/lib/wsRemote';
import { JSON_RPC_ERRORS, makeError } from './jsonRpc';
import {
  type ChannelTransport,
  type ControlFrame,
  type ControlListener,
  type JsonRpcId,
  type OutboundFrame,
  type PaneBytesListener,
  type StateListener,
  type TransportState,
  type Unsubscribe,
} from './types';

function mapState(s: ConnectionState): TransportState {
  // RemoteConnection has no distinct `reconnecting`; its reconnect path goes
  // through `connecting`. We surface `connecting` as-is — L2 only rejects
  // in-flight on a `connected → (reconnecting|disconnected|error)` edge, and
  // RemoteConnection emits `disconnected`/`error` on a drop, so the reject
  // semantics still fire correctly.
  return s;
}

export class LanWsAdapter implements ChannelTransport {
  private readonly conn: RemoteConnection;
  private controlListeners = new Set<ControlListener>();
  private paneListeners = new Set<PaneBytesListener>();
  private detachers: Unsubscribe[] = [];
  // §S3 negotiated upgrade: starts in legacy-translation mode so requests ride
  // the old `invoke-request`/`invoke-result` envelope (byte-for-byte unchanged).
  // `$/`-control methods (`$/hello`, `$/cancel`) ALWAYS pass through as native
  // JSON-RPC so the host's JSON-RPC leg handles them. When the host's `$/hello`
  // reply confirms it speaks JSON-RPC natively, we flip to `jsonRpcNative` and
  // pass ALL frames through untouched — so invoke errors then carry the full
  // `{code,message,data}` (the D-GM-2 fix on the LAN leg). A host that never
  // replies to `$/hello` (old build) keeps the legacy translation forever.
  private jsonRpcNative = false;

  constructor(conn: RemoteConnection) {
    this.conn = conn;
    this.detachers.push(
      conn.onMessage((msg) => this.handleInbound(msg as ControlFrame)),
    );
    this.detachers.push(conn.onRawBytes((paneId, bytes) => this.emitPaneBytes(paneId, bytes)));
  }

  /** Expose the wrapped connection for boot code that still drives it directly
   *  (auth handshake, theme cache). Adapter owns translation, not lifecycle. */
  get connection(): RemoteConnection {
    return this.conn;
  }

  // ── L1: control channel ─────────────────────────────────────────────────────
  sendControl(frame: OutboundFrame): void {
    this.conn.send(this.toWire(frame as ControlFrame));
  }

  onControl(cb: ControlListener): Unsubscribe {
    this.controlListeners.add(cb);
    return () => this.controlListeners.delete(cb);
  }

  // ── L1: pane bytes ──────────────────────────────────────────────────────────
  sendPaneBytes(_paneId: string, _bytes: Uint8Array): void {
    // The LAN leg never originates pane bytes from the client (PTY input rides a
    // `stdin` control message via the host). Kept for interface symmetry.
  }

  onPaneBytes(cb: PaneBytesListener): Unsubscribe {
    this.paneListeners.add(cb);
    return () => this.paneListeners.delete(cb);
  }

  // ── L1: lifecycle ───────────────────────────────────────────────────────────
  connect(): void {
    // RemoteConnection.connect(host, port, auth) is driven by the web-remote
    // boot (it owns host/port/token + the TOTP handshake). The adapter wraps an
    // already-connecting/connected RemoteConnection, so this is a no-op.
  }

  close(): void {
    this.conn.disconnect();
  }

  state(): TransportState {
    return mapState(this.conn.state());
  }

  onStateChange(cb: StateListener): Unsubscribe {
    return this.conn.onStateChange((s: ConnectionState) => cb(mapState(s)));
  }

  // ── translation ─────────────────────────────────────────────────────────────
  /** JSON-RPC (or already-legacy) frame → the wire object the host reads.
   *
   *  - A `$/`-control method (`$/hello`, `$/cancel`) is ALWAYS sent natively so
   *    the host's JSON-RPC leg handles it (the legacy leg has no concept of it).
   *  - Once `jsonRpcNative` (host confirmed JSON-RPC via its `$/hello` reply),
   *    EVERY JSON-RPC frame is sent natively → invoke errors carry full
   *    `{code,message,data}`.
   *  - Otherwise an invoke request/notification is translated to the legacy
   *    `invoke-request` / flat control frame (byte-for-byte unchanged). */
  private toWire(frame: ControlFrame): ControlFrame {
    if (frame.jsonrpc !== '2.0') return frame; // already a legacy control frame

    const { method } = frame as { method?: string };
    // `$/`-control + post-negotiation: forward the JSON-RPC envelope verbatim.
    if ((typeof method === 'string' && method.startsWith('$/')) || this.jsonRpcNative) {
      return frame;
    }

    const { id, params } = frame as {
      id?: JsonRpcId;
      params?: unknown;
    };
    // ($/-control methods, incl. `$/cancel`, already returned natively above.)

    // Request (has id + method) → invoke-request.
    if (typeof method === 'string' && id !== undefined) {
      return {
        type: 'invoke-request',
        cmd: method,
        args: (params as Record<string, unknown>) ?? {},
        _reqId: id,
      };
    }

    // Notification (method, no id).
    if (typeof method === 'string') {
      // Spread params so legacy control messages keep their flat shape
      // (e.g. `{type:'use-global-workspace'}`, `{type:'subscribe-pane', paneId}`).
      const flat = (params as Record<string, unknown> | undefined) ?? {};
      return { type: method, ...flat };
    }

    return frame;
  }

  /** Inbound frame → control frame for L2.
   *
   *  Three shapes can arrive:
   *    1. A native JSON-RPC frame (`jsonrpc:"2.0"`) — the S3 host's JSON-RPC leg
   *       (responses with `{code,message,data}` errors, `$/hello`/`$/bye`
   *       notifications). PASS THROUGH VERBATIM so the structured error reaches
   *       L2's `RpcRemoteError` intact (this is the D-GM-2 fix on the client
   *       side: no re-wrapping, full code/data preserved).
   *    2. A legacy `invoke-result` (older host, or any host replying to a frame
   *       we sent as legacy invoke-request) — translate to a JSON-RPC response.
   *       The legacy `_error` is a bare string, so it still degrades to
   *       INTERNAL_ERROR here; that is the unavoidable legacy-leg limit and is
   *       why the host JSON-RPC leg exists. Paired anchor:
   *       server.rs `core_result_to_envelope`.
   *    3. A generic host push (`{type:'event', …}`) — pass through verbatim.
   */
  private handleInbound(msg: ControlFrame): void {
    // (1) Native JSON-RPC frame from the S3 host — forward untouched.
    if (msg.jsonrpc === '2.0') {
      // The host's `$/hello` reply proves it speaks JSON-RPC natively → upgrade
      // so subsequent invoke requests ride the native envelope (full error
      // code/data). `$/bye` (version mismatch) does NOT upgrade.
      if (msg.method === '$/hello') {
        this.jsonRpcNative = true;
      }
      this.emitControl(msg);
      return;
    }
    // (2) Legacy invoke-result → JSON-RPC response.
    if (msg.type === 'invoke-result' && (typeof msg._reqId === 'number' || typeof msg._reqId === 'string')) {
      const id = msg._reqId as JsonRpcId;
      const frame: ControlFrame =
        msg._error !== undefined && msg._error !== null
          ? {
              jsonrpc: '2.0',
              id,
              error: makeError(JSON_RPC_ERRORS.INTERNAL_ERROR, String(msg._error)),
            }
          : { jsonrpc: '2.0', id, result: msg._result ?? null };
      this.emitControl(frame);
      return;
    }
    // (3) Generic host push (`{type:'event', name, payload}` etc.) → pass through
    // so callers that translate these (the bridge) still receive them verbatim.
    this.emitControl(msg);
  }

  private emitControl(frame: ControlFrame): void {
    for (const cb of this.controlListeners) {
      try {
        cb(frame);
      } catch (e) {
        console.error('[lanWsAdapter] control listener threw', e);
      }
    }
  }

  private emitPaneBytes(paneId: string, bytes: Uint8Array): void {
    for (const cb of this.paneListeners) {
      try {
        cb(paneId, bytes);
      } catch (e) {
        console.error('[lanWsAdapter] pane-bytes listener threw', e);
      }
    }
  }

  /** Detach all RemoteConnection listeners (does not disconnect the socket). */
  dispose(): void {
    for (const d of this.detachers) d();
    this.detachers = [];
    this.controlListeners.clear();
    this.paneListeners.clear();
  }
}

/** Factory matching the boot path: wrap an authenticated RemoteConnection. */
export function createLanWsTransport(conn: RemoteConnection): LanWsAdapter {
  return new LanWsAdapter(conn);
}
