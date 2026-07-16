// src/lib/transport/remote/jsonRpc.ts
//
// JSON-RPC 2.0 envelope helpers shared by the L2 RpcClient and any adapter that
// speaks JSON-RPC natively. Field layout is fixed by the S0 contract (see
// ./types.ts) and must stay byte-compatible with the host.

import type {
  ControlFrame,
  JsonRpcError,
  JsonRpcErrorResponse,
  JsonRpcId,
  JsonRpcNotification,
  JsonRpcRequest,
  JsonRpcResponse,
  JsonRpcSuccessResponse,
} from './types';

export function buildRequest(id: JsonRpcId, method: string, params?: unknown): JsonRpcRequest {
  const req: JsonRpcRequest = { jsonrpc: '2.0', id, method };
  if (params !== undefined) req.params = params;
  return req;
}

export function buildNotification(method: string, params?: unknown): JsonRpcNotification {
  const note: JsonRpcNotification = { jsonrpc: '2.0', method };
  if (params !== undefined) note.params = params;
  return note;
}

/** A response carries an `id` and exactly one of `result` / `error`. */
export function isJsonRpcResponse(frame: ControlFrame): frame is ControlFrame & JsonRpcResponse {
  if (frame.jsonrpc !== '2.0') return false;
  const id = frame.id;
  const hasId = typeof id === 'number' || typeof id === 'string';
  if (!hasId) return false;
  return 'result' in frame || 'error' in frame;
}

export function isErrorResponse(
  frame: JsonRpcResponse,
): frame is JsonRpcErrorResponse {
  return 'error' in frame && (frame as JsonRpcErrorResponse).error != null;
}

export function isSuccessResponse(
  frame: JsonRpcResponse,
): frame is JsonRpcSuccessResponse {
  return !isErrorResponse(frame);
}

/** A notification is a JSON-RPC frame with a `method` and no `id`. */
export function isJsonRpcNotification(
  frame: ControlFrame,
): frame is ControlFrame & JsonRpcNotification {
  return (
    frame.jsonrpc === '2.0' &&
    typeof frame.method === 'string' &&
    frame.id === undefined
  );
}

export function makeError(code: number, message: string, data?: unknown): JsonRpcError {
  const err: JsonRpcError = { code, message };
  if (data !== undefined) err.data = data;
  return err;
}

// Standard JSON-RPC 2.0 error codes (subset used here).
export const JSON_RPC_ERRORS = {
  PARSE_ERROR: -32700,
  INVALID_REQUEST: -32600,
  METHOD_NOT_FOUND: -32601,
  INVALID_PARAMS: -32602,
  INTERNAL_ERROR: -32603,
} as const;
