export type { DataProvider, GitStatusResult, SearchResult } from './types';
export { setTransport, getTransport, hasTransport } from './context';
export { TauriDataProvider } from './tauri';
export { WsDataProvider } from './ws';

// Two-layer remote Transport abstraction (L1 channel primitives + L2 RPC).
export type {
  ChannelTransport,
  ControlFrame,
  TransportState,
  JsonRpcRequest,
  JsonRpcResponse,
  JsonRpcError,
  RpcRequestOptions,
} from '@ridge/remote';
export {
  RpcReconnectError,
  RpcTimeoutError,
  RpcCancelledError,
  RpcRemoteError,
} from '@ridge/remote';
export { RpcClient } from '@ridge/remote';
export { LanWsAdapter, createLanWsTransport } from '@ridge/remote';
export {
  CloudWebrtcAdapter,
  createCloudWebrtcTransport,
  createCloudWebrtcTransportWith,
} from '@ridge/remote';
export { CHANNEL, encodeJsonFrame, encodePaneFrame, demuxFrame } from '@ridge/remote';