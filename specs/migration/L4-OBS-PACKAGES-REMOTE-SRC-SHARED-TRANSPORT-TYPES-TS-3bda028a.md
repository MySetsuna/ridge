---
id: L4-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-TYPES-TS-3bda028a
level: L4
parent: L3-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-a4b7a712
title: types.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/remote/src/shared/transport/types.ts
test_targets:
  - packages/remote/src/shared/transport/capabilityContract.test.ts
  - packages/remote/src/shared/transport/cloudChunk.test.ts
  - packages/remote/src/shared/transport/cloudMux.test.ts
  - packages/remote/src/shared/transport/cloudWebrtcAdapter.test.ts
  - packages/remote/src/shared/transport/conformance.test.ts
  - packages/remote/src/shared/transport/lanWsAdapter.test.ts
  - packages/remote/src/shared/transport/matrixParity.test.ts
  - packages/remote/src/shared/transport/paneRpcScheduler.test.ts
  - packages/remote/src/shared/transport/protocolAdmission.test.ts
  - packages/remote/src/shared/transport/protocolAdmissionProduct.test.ts
  - packages/remote/src/shared/transport/random.test.ts
  - packages/remote/src/shared/transport/remoteInvokeAdmit.test.ts
  - packages/remote/src/shared/transport/remotePerfTrace.test.ts
  - packages/remote/src/shared/transport/rpcClient.test.ts
  - packages/remote/src/shared/transport/unknownText.test.ts
  - packages/remote/src/shared/transport/wsRemote.behavior.test.ts
  - packages/remote/src/shared/transport/wsRemotePending.test.ts
  - packages/remote/src/shared/transport/wsRemoteRpcScheduler.test.ts
  - packages/remote/src/shared/transport/wsRemoteUrl.test.ts
public_interface:
  - export class RpcCancelledError
  - export class RpcQueueFullError
  - export class RpcReconnectError
  - export class RpcRemoteError
  - export class RpcTimeoutError
  - export interface ChannelTransport
  - export interface JsonRpcError
  - export interface JsonRpcErrorResponse
  - export interface JsonRpcNotification
  - export interface JsonRpcRequest
  - export interface JsonRpcSuccessResponse
  - export interface RpcRequestOptions
  - export type AuthListener
  - export type AuthState
  - export type ControlFrame
  - export type ControlListener
  - export type JsonRpcId
  - export type JsonRpcResponse
  - export type OutboundFrame
  - export type PaneBytesListener
  - export type StateListener
  - export type TransportState
  - export type Unsubscribe
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-CAPABILITYCONTRACT-TEST-TS-b0bfbedf
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-CLOUDCHUNK-TEST-TS-cd20cdb9
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-CLOUDMUX-TEST-TS-2d7e0111
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-CLOUDWEBRTCADAPTER-TEST-TS-8470c932
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-CONFORMANCE-TEST-TS-6908b6ac
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-LANWSADAPTER-TEST-TS-a2b9af4c
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-MATRIXPARITY-TEST-TS-d457ef87
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-PANERPCSCHEDULER-TEST-TS-ad772920
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-PROTOCOLADMISSION-TEST-TS-535b3e0d
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-PROTOCOLADMISSIONPRODUCT-TEST-TS-1940534e
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-RANDOM-TEST-TS-d885afcc
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-REMOTEINVOKEADMIT-TEST-TS-d1037371
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-REMOTEPERFTRACE-TEST-TS-a6430a11
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-RPCCLIENT-TEST-TS-605f379d
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-UNKNOWNTEXT-TEST-TS-ffdcdbcf
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-WSREMOTE-BEHAVIOR-TEST-TS-8b7a590c
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-WSREMOTEPENDING-TEST-TS-ee9c7d29
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-WSREMOTERPCSCHEDULER-TEST-TS-04be1fed
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-WSREMOTEURL-TEST-TS-a219c8e6
---

# types.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
