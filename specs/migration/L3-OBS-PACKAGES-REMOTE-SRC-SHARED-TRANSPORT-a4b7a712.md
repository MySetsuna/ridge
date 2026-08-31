---
id: L3-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-a4b7a712
level: L3
parent: L2-OBS-PACKAGES-b28b1ed9
title: packages/remote/src/shared/transport module
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/remote/src/shared/transport/capabilityContract.ts
  - packages/remote/src/shared/transport/cloudChunk.ts
  - packages/remote/src/shared/transport/cloudMux.ts
  - packages/remote/src/shared/transport/cloudWebrtcAdapter.ts
  - packages/remote/src/shared/transport/connectionProvider.ts
  - packages/remote/src/shared/transport/deviceId.ts
  - packages/remote/src/shared/transport/jsonRpc.ts
  - packages/remote/src/shared/transport/lanWsAdapter.ts
  - packages/remote/src/shared/transport/matrixParity.ts
  - packages/remote/src/shared/transport/paneRef.ts
  - packages/remote/src/shared/transport/paneRpcScheduler.ts
  - packages/remote/src/shared/transport/protocolAdmission.ts
  - packages/remote/src/shared/transport/random.ts
  - packages/remote/src/shared/transport/remoteInvokeAdmit.ts
  - packages/remote/src/shared/transport/remotePerfTrace.ts
  - packages/remote/src/shared/transport/rpcClient.ts
  - packages/remote/src/shared/transport/types.ts
  - packages/remote/src/shared/transport/unknownText.ts
  - packages/remote/src/shared/transport/wsRemote.ts
public_interface:
  - "export async function remotePerfSamplePeerConnection( pc:
    Pick<RTCPeerConnection, 'getStats'>, ): Promise<void>"
  - export class ChunkReassembler
  - export class CloudWebrtcAdapter
  - export class LanWsAdapter
  - export class PaneInputQueueFullError
  - export class PaneRpcScheduler
  - export class RemoteConnection
  - export class RpcCancelledError
  - export class RpcClient
  - export class RpcQueueFullError
  - export class RpcReconnectError
  - export class RpcRemoteError
  - export class RpcTimeoutError
  - "export function admitDesktopMethod(method: string): AdmitResult"
  - "export function admitRemoteMethod(method: string): AdmitResult"
  - "export function allMatrixMethods(matrix: MatrixDoc): string[]"
  - "export function buildNotification(method: string, params?: unknown):
    JsonRpcNotification"
  - "export function buildRequest(id: JsonRpcId, method: string, params?:
    unknown): JsonRpcRequest"
  - "export function canonicalizeMethod(method: string): string"
  - "export function capabilityForRemoteMethod(method: string): RemoteCapability
    | undefined"
  - "export function capabilityFullyAdmitted( capability: keyof typeof
    REMOTE_CAPABILITY_METHODS, allowlist: readonly string[], ): boolean"
  - "export function classifyFailure(code?: string, closeCode?: number):
    ConnectionFailure"
  - "export function createCloudWebrtcTransport( provider:
    RemoteConnectionProvider, deviceId: string, ): CloudWebrtcAdapter"
  - "export function createCloudWebrtcTransportWith( deviceId: string,
    makeProvider: (callbacks: CloudConnectionCallbacks)"
  - "export function createLanWsTransport(conn: RemoteConnection): LanWsAdapter"
  - "export function decideRemoteInvoke(rawMethod: string): RemoteInvokeDecision"
  - "export function demuxFrame(frame: Uint8Array): DemuxResult"
  - "export function deniedControllerMethods( capability: keyof typeof
    REMOTE_CAPABILITY_METHODS, allowlist: readonly string[], ): string[]"
  - "export function encodeChunks(ciphertext: Uint8Array, msgId: number):
    Uint8Array[]"
  - "export function encodeControlFrame(value: unknown): Uint8Array"
  - "export function encodeJsonFrame(value: unknown): Uint8Array"
  - "export function encodePaneFrame(paneId: string, bytes: Uint8Array):
    Uint8Array"
  - "export function encodePaneLaneProbeFrame(): Uint8Array"
  - "export function encodePaneLaneReadyFrame(): Uint8Array"
  - "export function filterAdmittedMethods(methods: string[]):"
  - "export function forbiddenPresent(allowlist: string[], forbidden: readonly
    string[]): string[]"
  - "export function getRemoteDeviceId(): string"
  - "export function getRemotePanelAvailability( hasCapability: (capability:
    RemoteCapability)"
  - "export function isDesktopPrivileged(method: string): boolean"
  - "export function isErrorResponse( frame: JsonRpcResponse, ): frame is
    JsonRpcErrorResponse"
  - "export function isJsonRpcNotification( frame: ControlFrame, ): frame is
    ControlFrame & JsonRpcNotification"
  - "export function isJsonRpcResponse(frame: ControlFrame): frame is
    ControlFrame & JsonRpcResponse"
  - "export function isPaneLaneProbeFrame(frame: Uint8Array): boolean"
  - "export function isPaneLaneReadyFrame(frame: Uint8Array): boolean"
  - "export function isSuccessResponse( frame: JsonRpcResponse, ): frame is
    JsonRpcSuccessResponse"
  - "export function isValidMethodName(method: string): boolean"
  - "export function makeError(code: number, message: string, data?: unknown):
    JsonRpcError"
  - "export function methodCategory( method: string, ): 'desktop_host' |
    'teammate' | 'workspace' | 'terminal' | 'other'"
  - "export function missingRequired(allowlist: string[], required: readonly
    string[]): string[]"
  - "export function paneRefKey(ref: PaneRef): string"
  - "export function pendingKey(responseType: string, reqId: unknown): string"
  - "export function remoteMayInvoke(method: string, isRemoteController:
    boolean): boolean"
  - "export function remotePerfEnd(token: RemotePerfToken | null, meta?:
    RemotePerfMeta): void"
  - "export function remotePerfMark( stage: RemotePerfStage, meta?:
    RemotePerfMeta, ): void"
  - "export function remotePerfSnapshot(): RemotePerfSnapshot"
  - "export function remotePerfStart( stage: RemotePerfStage, meta?:
    RemotePerfMeta, ): RemotePerfToken | null"
  - "export function remoteWebSocketUrl(input: { host: string; port: number;
    auth: string; authType: 'code' | 'token'; device: string; secure: boolean;
    }): string"
  - "export function reportMatrixParity( allowlist: readonly string[], matrix:
    MatrixDoc, ): ParityReport"
  - "export function resetRemotePerfTrace(): void"
  - "export function secureRandomUnit(): number"
  - "export function teammateFromMatrix(matrix: MatrixDoc): string[]"
  - "export function unknownText(value: unknown, fallback = ''): string"
  - "export function validateTeammateHostsBoundary(allowlist: string[]):"
  - export interface AgentHistoryReply
  - export interface AgentMessageReceipt
  - export interface AgentMessageTarget
  - export interface ChannelTransport
  - export interface CloudConnectionCallbacks
  - export interface ConnectionFailure
  - export interface FileEntry
  - export interface GitStatus
  - export interface HitlPendingItem
  - export interface JsonRpcError
  - export interface JsonRpcErrorResponse
  - export interface JsonRpcNotification
  - export interface JsonRpcRequest
  - export interface JsonRpcSuccessResponse
  - export interface MatrixDoc
  - export interface NegotiatedProtocol
  - export interface OrchestrationHealth
  - export interface PaneInfo
  - export interface PaneRef
  - export interface PaneRpcSchedulerDiagnostics
  - export interface PaneRpcSchedulerOptions
  - export interface ParityReport
  - export interface PendingScrollbackPage
  - export interface RemoteConnectionProvider
  - export interface RemoteLink
  - export interface RemotePerfSample
  - export interface RemotePerfSnapshot
  - export interface RemotePerfToken
  - export interface RemoteShellInfo
  - export interface RpcClientOptions
  - export interface RpcDiagnostics
  - export interface RpcRequestOptions
  - export interface SavedWorkspaceFile
  - export interface TeammateGroup
  - export interface TeammateRosterMember
  - export interface TeammateTopology
  - export interface ThemeSnapshot
  - export interface WorkspaceInfo
  - export type AdmitResult
  - export type AuthListener
  - export type AuthState
  - export type CloudConnectionState
  - export type ConnectionFailureCategory
  - export type ConnectionState
  - export type ControlFrame
  - export type ControlListener
  - export type DemuxResult
  - export type HitlResolveOutcome
  - export type JsonRpcId
  - export type JsonRpcResponse
  - export type MetaListener
  - export type NotificationHandler
  - export type OutboundFrame
  - export type PaneBytesListener
  - export type PaneRenderOwner
  - export type PtyResizeListener
  - export type RawByteListener
  - export type RemoteCapability
  - export type RemoteInvokeDecision
  - export type RemotePanel
  - export type RemotePerfStage
  - export type ResyncHook
  - export type StateListener
  - export type ThemeListener
  - export type TransportState
  - export type Unsubscribe
  - export type WsMessage
---

# packages/remote/src/shared/transport module

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
