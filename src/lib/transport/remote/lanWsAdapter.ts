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
  CANCEL_METHOD,
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
  /** JSON-RPC (or already-legacy) frame → the legacy wire object the host reads. */
  private toWire(frame: ControlFrame): ControlFrame {
    if (frame.jsonrpc !== '2.0') return frame; // already a legacy control frame

    const { method, id, params } = frame as {
      method?: string;
      id?: JsonRpcId;
      params?: unknown;
    };

    // Request (has id + method) → invoke-request.
    if (typeof method === 'string' && id !== undefined) {
      if (method === CANCEL_METHOD) {
        const target = (params as { id?: JsonRpcId } | undefined)?.id;
        return { type: 'cancel', _reqId: target };
      }
      return {
        type: 'invoke-request',
        cmd: method,
        args: (params as Record<string, unknown>) ?? {},
        _reqId: id,
      };
    }

    // Notification (method, no id).
    if (typeof method === 'string') {
      if (method === CANCEL_METHOD) {
        const target = (params as { id?: JsonRpcId } | undefined)?.id;
        return { type: 'cancel', _reqId: target };
      }
      // Spread params so legacy control messages keep their flat shape
      // (e.g. `{type:'use-global-workspace'}`, `{type:'subscribe-pane', paneId}`).
      const flat = (params as Record<string, unknown> | undefined) ?? {};
      return { type: method, ...flat };
    }

    return frame;
  }

  /** Inbound legacy frame → JSON-RPC response (for L2) or pass-through. */
  private handleInbound(msg: ControlFrame): void {
    if (msg.type === 'invoke-result' && (typeof msg._reqId === 'number' || typeof msg._reqId === 'string')) {
      const id = msg._reqId as JsonRpcId;
      // TODO(S3): legacy `_error` is a bare string, so ridge-core's structured
      // JSON-RPC code/data is lost and everything degrades to INTERNAL_ERROR
      // (-32603). Once S3 makes the LAN host JSON-RPC-native it will send a real
      // `{code,message,data}` error object — forward it verbatim instead of
      // re-wrapping here. Paired anchor: server.rs `core_result_to_envelope`.
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
    // Generic host push (`{type:'event', name, payload}` etc.) → pass through so
    // callers that translate these (the bridge) still receive them verbatim.
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
