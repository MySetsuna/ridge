// src/lib/transport/remote/types.ts
//
// Two-layer client Transport abstraction for the unified remote architecture
// (handoff plan §5.3, decisions D6/D7). Splitting "channel primitives" (L1)
// from the "shared RPC client" (L2) means reqId correlation / timeout / cancel
// / reconnect semantics are written ONCE in L2 and ride any adapter, instead of
// each adapter (LAN-WS, cloud-WebRTC) re-implementing them and drifting (D7).
//
//   L1  ChannelTransport  — per-adapter primitives: control send/recv,
//                           pane-bytes send/recv, lifecycle + state.
//   L2  RpcClient         — written once, runs on L1's control channel:
//                           request()/cancel()/subscriptions/reconnect.
//
// JSON-RPC 2.0 is the on-the-wire control envelope (shared with the S0 contract).
// Adapters that wrap a host which does NOT yet speak JSON-RPC (the LAN-WS host
// today) translate at the L1 boundary, so L2 stays pure JSON-RPC and forward-
// compatible with the cloud-WebRTC leg where the host speaks it natively.

// ── JSON-RPC 2.0 envelope (verbatim per S2 task / S0 contract) ──────────────
// request  { "jsonrpc":"2.0", "id":<num|str>, "method":<str>, "params":<obj> }
// success  { "jsonrpc":"2.0", "id":..., "result":... }
// error    { "jsonrpc":"2.0", "id":..., "error":{ "code":<int>,"message":<str>,"data"?:... } }
// notify   (no "id")
// cancel   method "$/cancel", params { "id":<target id> }

export type JsonRpcId = number | string;

export interface JsonRpcRequest {
  jsonrpc: '2.0';
  id: JsonRpcId;
  method: string;
  params?: unknown;
}

export interface JsonRpcNotification {
  jsonrpc: '2.0';
  method: string;
  params?: unknown;
}

export interface JsonRpcError {
  code: number;
  message: string;
  data?: unknown;
}

export interface JsonRpcSuccessResponse {
  jsonrpc: '2.0';
  id: JsonRpcId;
  result: unknown;
}

export interface JsonRpcErrorResponse {
  jsonrpc: '2.0';
  id: JsonRpcId;
  error: JsonRpcError;
}

export type JsonRpcResponse = JsonRpcSuccessResponse | JsonRpcErrorResponse;

/** Any frame received on the control channel as a parsed JSON object. */
export type ControlFrame = Record<string, unknown>;

/** A frame that can be *sent* on the control channel: a structured JSON-RPC
 *  envelope (what L2 emits) or an arbitrary legacy control frame (what a
 *  pre-JSON-RPC adapter caller may pass). Adapters narrow as needed. */
export type OutboundFrame =
  | JsonRpcRequest
  | JsonRpcNotification
  | JsonRpcResponse
  | ControlFrame;

/** Cancel notification method name (LSP-style `$/cancel`, per S2 task spec). */
export const CANCEL_METHOD = '$/cancel';

// ── Connection lifecycle ────────────────────────────────────────────────────
// Mirrors the existing RemoteConnection states plus an explicit `reconnecting`
// so L2 can reject in-flight requests + re-subscribe on the transition.
export type TransportState =
  | 'connecting'
  | 'connected'
  | 'reconnecting'
  | 'disconnected'
  | 'error';

export type ControlListener = (frame: ControlFrame) => void;
export type PaneBytesListener = (paneId: string, bytes: Uint8Array) => void;
export type StateListener = (state: TransportState) => void;
export type Unsubscribe = () => void;

// ── L1: channel primitives (each adapter implements) ────────────────────────
// Adapters own wire framing, mux, auth handshake and (for the LAN-WS host that
// predates JSON-RPC) any envelope translation. L2 only ever sees parsed control
// frames and raw pane bytes.
export interface ChannelTransport {
  /** Send a control frame. For JSON-RPC adapters this is the request/
   *  notification envelope verbatim; legacy adapters translate at this boundary. */
  sendControl(frame: OutboundFrame): void;
  /** Subscribe to inbound control frames (parsed JSON objects). */
  onControl(cb: ControlListener): Unsubscribe;

  /** Push raw PTY bytes for a pane (high-frequency, one-way). */
  sendPaneBytes(paneId: string, bytes: Uint8Array): void;
  /** Subscribe to inbound raw pane bytes (the binary fan-out). */
  onPaneBytes(cb: PaneBytesListener): Unsubscribe;

  /** Begin connecting (incl. the adapter's own auth handshake). */
  connect(): void | Promise<void>;
  /** Tear the connection down. */
  close(): void;
  /** Current connection state. */
  state(): TransportState;
  /** Subscribe to lifecycle/reconnect transitions. */
  onStateChange(cb: StateListener): Unsubscribe;
}

// ── L2: shared RPC client surface ───────────────────────────────────────────
export interface RpcRequestOptions {
  /** Per-request timeout in ms. Falls back to the client default. */
  timeoutMs?: number;
  /** AbortSignal — aborting sends a `$/cancel` and rejects the promise. */
  signal?: AbortSignal;
}

/** Error thrown when a request is rejected because the transport reconnected
 *  (all in-flight requests reject; the caller decides whether to retry). */
export class RpcReconnectError extends Error {
  constructor(method: string) {
    super(`rpc('${method}') rejected: transport reconnected`);
    this.name = 'RpcReconnectError';
  }
}

/** Error thrown when a request exceeds its timeout. */
export class RpcTimeoutError extends Error {
  constructor(method: string, timeoutMs: number) {
    super(`rpc('${method}') timed out after ${timeoutMs}ms`);
    this.name = 'RpcTimeoutError';
  }
}

/** Error thrown when a request is cancelled via AbortSignal / cancel(). */
export class RpcCancelledError extends Error {
  constructor(method: string) {
    super(`rpc('${method}') cancelled`);
    this.name = 'RpcCancelledError';
  }
}

/** Error carrying a JSON-RPC `error` object returned by the host. */
export class RpcRemoteError extends Error {
  code: number;
  data?: unknown;
  constructor(method: string, error: JsonRpcError) {
    super(error.message || `rpc('${method}') failed`);
    this.name = 'RpcRemoteError';
    this.code = error.code;
    this.data = error.data;
  }
}
