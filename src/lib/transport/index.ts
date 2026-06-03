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
} from './remote/types';
export {
  RpcReconnectError,
  RpcTimeoutError,
  RpcCancelledError,
  RpcRemoteError,
} from './remote/types';
export { RpcClient } from './remote/rpcClient';
export { LanWsAdapter, createLanWsTransport } from './remote/lanWsAdapter';
export {
  CloudWebrtcAdapter,
  createCloudWebrtcTransport,
  createCloudWebrtcTransportWith,
} from './remote/cloudWebrtcAdapter';
export { CHANNEL, encodeJsonFrame, encodePaneFrame, demuxFrame } from './remote/cloudMux';