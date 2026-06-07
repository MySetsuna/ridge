// src/lib/transport/remote/rpcClient.ts
//
// L2 shared RPC client (handoff plan §5.3, D7). Written ONCE and runs on any L1
// ChannelTransport's control channel — adapters never re-implement correlation.
//
// Responsibilities:
//   • request(method, params) → Promise, correlated by a JSON-RPC `id`.
//   • per-request timeout (rejects with RpcTimeoutError).
//   • cancel(id) / AbortSignal → sends `$/cancel` + rejects (RpcCancelledError).
//   • subscriptions: register intent so the bridge can re-subscribe on reconnect.
//   • reconnect: on `reconnecting`, ALL in-flight requests reject
//     (RpcReconnectError) — never silently replayed; on `connected` again it
//     re-runs the registered resync hooks (re-subscribe panes + re-pull D10
//     snapshots).  D9 handshake is consumed via onControl notifications.
//
// L2 is pure JSON-RPC: it emits envelopes through sendControl and parses
// responses/notifications from onControl. Wire translation for legacy hosts
// lives in the L1 adapter (see lanWsAdapter.ts), keeping this layer reusable
// across LAN-WS and cloud-WebRTC.

import {
  buildNotification,
  buildRequest,
  isErrorResponse,
  isJsonRpcNotification,
  isJsonRpcResponse,
} from './jsonRpc';
import {
  CANCEL_METHOD,
  RpcCancelledError,
  RpcReconnectError,
  RpcRemoteError,
  RpcTimeoutError,
  type ChannelTransport,
  type ControlFrame,
  type JsonRpcId,
  type RpcRequestOptions,
  type TransportState,
  type Unsubscribe,
} from './types';

const DEFAULT_TIMEOUT_MS = 20_000;

/** D9 handshake method name (contract §7.3). */
export const HELLO_METHOD = '$/hello';
/** D9 teardown method name (contract §7.3): no common protocol version. */
export const BYE_METHOD = '$/bye';
/** Protocol version this client implements (D9). */
export const CLIENT_PROTOCOL_VERSION = 1;
/** Capabilities this client (controller SPA) can drive (contract §7.3). */
export const CLIENT_CAPABILITIES = [
  'pane',
  'invoke',
  'fs',
  'git',
  'search',
  'workspace',
  'theme',
] as const;

/** Result of the D9 `$/hello` negotiation, surfaced to the bridge so it can
 *  grey out panels for capabilities the host does not serve. */
export interface NegotiatedProtocol {
  /** Highest common protocol version the host agreed to. */
  protocolVersion: number;
  /** Capability intersection (host ∩ client). Panels outside this are hidden. */
  capabilities: Set<string>;
  /** True if the host sent `$/bye` (version mismatch) — the SPA should prompt
   *  the user to upgrade. */
  rejected: boolean;
  /** Reason supplied with `$/bye`, if any. */
  reason?: string;
}

interface Pending {
  method: string;
  resolve: (v: unknown) => void;
  reject: (e: Error) => void;
  timer: ReturnType<typeof setTimeout> | null;
  signal?: AbortSignal;
  onAbort?: () => void;
}

/** A notification handler keyed by method name (host → client pushes). */
export type NotificationHandler = (params: unknown) => void;

/** A resync hook run after a reconnect completes (re-subscribe + re-pull). */
export type ResyncHook = () => void;

export interface RpcClientOptions {
  /** Default per-request timeout (ms). */
  defaultTimeoutMs?: number;
  /** Optional id factory (e.g. for deterministic tests). Default: incrementing. */
  nextId?: () => JsonRpcId;
}

export class RpcClient {
  private readonly transport: ChannelTransport;
  private readonly defaultTimeoutMs: number;
  private readonly nextId: () => JsonRpcId;

  private pending = new Map<JsonRpcId, Pending>();
  private notificationHandlers = new Map<string, Set<NotificationHandler>>();
  private resyncHooks = new Set<ResyncHook>();
  private seq = 0;
  private prevState: TransportState;
  private disposers: Unsubscribe[] = [];

  // ── D9 handshake state ──
  private negotiated: NegotiatedProtocol | null = null;
  private negotiatedListeners = new Set<(p: NegotiatedProtocol) => void>();
  private helloSent = false;

  constructor(transport: ChannelTransport, opts: RpcClientOptions = {}) {
    this.transport = transport;
    this.defaultTimeoutMs = opts.defaultTimeoutMs ?? DEFAULT_TIMEOUT_MS;
    this.nextId = opts.nextId ?? (() => ++this.seq);
    this.prevState = transport.state();

    this.disposers.push(transport.onControl((f) => this.handleControl(f)));
    this.disposers.push(transport.onStateChange((s) => this.handleStateChange(s)));
    // D9 handshake is initiated explicitly by the boot code via `hello()` (so
    // ordering vs. the initial control frames is well-defined), then re-run
    // automatically after each reconnect in `handleStateChange`.
  }

  /** Issue a JSON-RPC request. Rejects on timeout, cancel, reconnect, or a
   *  JSON-RPC `error` from the host. */
  request<T = unknown>(
    method: string,
    params?: unknown,
    options: RpcRequestOptions = {},
  ): Promise<T> {
    const id = this.nextId();
    const timeoutMs = options.timeoutMs ?? this.defaultTimeoutMs;

    return new Promise<T>((resolve, reject) => {
      // Already-aborted signal → reject before touching the wire.
      if (options.signal?.aborted) {
        reject(new RpcCancelledError(method));
        return;
      }

      const timer =
        timeoutMs > 0
          ? setTimeout(() => {
              this.settle(id, () => reject(new RpcTimeoutError(method, timeoutMs)));
            }, timeoutMs)
          : null;

      const onAbort = () => {
        this.settle(id, () => reject(new RpcCancelledError(method)));
        this.sendCancel(id);
      };
      if (options.signal) options.signal.addEventListener('abort', onAbort, { once: true });

      this.pending.set(id, {
        method,
        resolve: (v) => resolve(v as T),
        reject,
        timer,
        signal: options.signal,
        onAbort,
      });

      this.transport.sendControl(buildRequest(id, method, params));
    });
  }

  /** Explicitly cancel an in-flight request by id (sends `$/cancel`). */
  cancel(id: JsonRpcId): void {
    this.settle(id, (p) => p.reject(new RpcCancelledError(p.method)));
    this.sendCancel(id);
  }

  /** Fire-and-forget JSON-RPC notification (no id, no response expected). */
  notify(method: string, params?: unknown): void {
    this.transport.sendControl(buildNotification(method, params));
  }

  /** Subscribe to host-pushed notifications for a given method. */
  onNotification(method: string, handler: NotificationHandler): Unsubscribe {
    let set = this.notificationHandlers.get(method);
    if (!set) {
      set = new Set();
      this.notificationHandlers.set(method, set);
    }
    set.add(handler);
    return () => this.notificationHandlers.get(method)?.delete(handler);
  }

  /** Register a hook run after the transport reconnects (re-subscribe panes,
   *  re-pull D10 snapshots). Returns an unsubscribe. */
  onReconnected(hook: ResyncHook): Unsubscribe {
    this.resyncHooks.add(hook);
    return () => this.resyncHooks.delete(hook);
  }

  // ── D9 version / capability handshake ──────────────────────────────────────
  /** Initiate the D9 handshake: send this client's `$/hello`. Idempotent per
   *  connection (a second call before a reconnect is a no-op). Called once by
   *  the boot code after the transport is attached; reconnects re-handshake
   *  automatically. */
  hello(): void {
    this.sendHello();
  }

  /** The negotiated protocol (capabilities + version), or `null` until the host
   *  has replied to `$/hello`. The bridge reads this to grey out unsupported
   *  IDE panels. */
  get protocol(): NegotiatedProtocol | null {
    return this.negotiated;
  }

  /** True if the host advertised `capability` in the `$/hello` intersection.
   *  Returns `true` while the handshake is still pending (optimistic — panels
   *  appear until the host says otherwise), and `false` if the host rejected us. */
  hasCapability(capability: string): boolean {
    if (this.negotiated === null) return true;
    if (this.negotiated.rejected) return false;
    return this.negotiated.capabilities.has(capability);
  }

  /** Subscribe to the D9 negotiation result. Fires once the host replies to
   *  `$/hello` (or `$/bye`), and again after a reconnect re-handshake. If a
   *  result is already known, the listener is called immediately. */
  onNegotiated(cb: (p: NegotiatedProtocol) => void): Unsubscribe {
    this.negotiatedListeners.add(cb);
    if (this.negotiated) cb(this.negotiated);
    return () => this.negotiatedListeners.delete(cb);
  }

  /** Number of in-flight requests (for tests / diagnostics). */
  get inFlight(): number {
    return this.pending.size;
  }

  /** Detach from the transport and reject any in-flight requests. */
  dispose(): void {
    for (const d of this.disposers) d();
    this.disposers = [];
    this.rejectAllInFlight(() => new RpcReconnectError('dispose'));
  }

  // ── internal ───────────────────────────────────────────────────────────────
  private sendCancel(id: JsonRpcId): void {
    this.transport.sendControl(buildNotification(CANCEL_METHOD, { id }));
  }

  /** Resolve/reject + clean up one pending entry by id. No-op if unknown. */
  private settle(id: JsonRpcId, run: (p: Pending) => void): void {
    const p = this.pending.get(id);
    if (!p) return;
    this.pending.delete(id);
    if (p.timer) clearTimeout(p.timer);
    if (p.signal && p.onAbort) p.signal.removeEventListener('abort', p.onAbort);
    run(p);
  }

  private handleControl(frame: ControlFrame): void {
    if (isJsonRpcResponse(frame)) {
      this.settle(frame.id, (p) => {
        if (isErrorResponse(frame)) {
          p.reject(new RpcRemoteError(p.method, frame.error));
        } else {
          p.resolve(frame.result);
        }
      });
      return;
    }
    if (isJsonRpcNotification(frame)) {
      // D9 handshake replies are notifications too; intercept before the
      // generic notification dispatch so consumers don't see them as data.
      if (frame.method === HELLO_METHOD) {
        this.handleHello(frame.params, false);
        return;
      }
      if (frame.method === BYE_METHOD) {
        this.handleHello(frame.params, true);
        return;
      }
      this.dispatchNotification(frame.method, frame.params);
    }
  }

  /** Send this client's `$/hello` (D9). Idempotent per connection. */
  private sendHello(): void {
    if (this.helloSent) return;
    this.helloSent = true;
    this.transport.sendControl(
      buildNotification(HELLO_METHOD, {
        protocolVersion: CLIENT_PROTOCOL_VERSION,
        capabilities: [...CLIENT_CAPABILITIES],
      }),
    );
  }

  /** Process the host's `$/hello` (or `$/bye`) reply and store the negotiated
   *  capability intersection. */
  private handleHello(params: unknown, rejected: boolean): void {
    const p = (params ?? {}) as { protocolVersion?: number; capabilities?: unknown; reason?: string };
    const caps = Array.isArray(p.capabilities)
      ? new Set(p.capabilities.filter((c): c is string => typeof c === 'string'))
      : new Set<string>();
    this.negotiated = {
      protocolVersion: typeof p.protocolVersion === 'number' ? p.protocolVersion : 0,
      capabilities: caps,
      rejected,
      reason: typeof p.reason === 'string' ? p.reason : undefined,
    };
    for (const cb of this.negotiatedListeners) {
      try {
        cb(this.negotiated);
      } catch (e) {
        console.error('[rpcClient] negotiated listener threw', e);
      }
    }
  }

  private dispatchNotification(method: string, params: unknown): void {
    const set = this.notificationHandlers.get(method);
    if (!set || set.size === 0) return;
    for (const h of set) {
      try {
        h(params);
      } catch (e) {
        console.error(`[rpcClient] notification handler for "${method}" threw`, e);
      }
    }
  }

  private handleStateChange(state: TransportState): void {
    const prev = this.prevState;
    this.prevState = state;

    // Leaving a live connection (reconnecting / dropped) → reject in-flight.
    // Requests are never silently replayed; upper layers retry idempotently.
    if (
      (state === 'reconnecting' || state === 'disconnected' || state === 'error') &&
      prev === 'connected'
    ) {
      this.rejectAllInFlight((m) => new RpcReconnectError(m));
    }

    // Re-established → D9 re-handshake (the SPA may have been updated
    // independently of the host, so re-negotiate), then run resync hooks
    // (re-subscribe + re-pull D10 snapshots).
    if (state === 'connected' && prev !== 'connected') {
      this.helloSent = false;
      this.sendHello();
      for (const hook of this.resyncHooks) {
        try {
          hook();
        } catch (e) {
          console.error('[rpcClient] resync hook threw', e);
        }
      }
    }
  }

  private rejectAllInFlight(makeError: (method: string) => Error): void {
    const entries = [...this.pending.values()];
    this.pending.clear();
    for (const p of entries) {
      if (p.timer) clearTimeout(p.timer);
      if (p.signal && p.onAbort) p.signal.removeEventListener('abort', p.onAbort);
      p.reject(makeError(p.method));
    }
  }
}
