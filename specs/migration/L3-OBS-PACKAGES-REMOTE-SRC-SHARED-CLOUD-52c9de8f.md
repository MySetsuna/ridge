---
id: L3-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-52c9de8f
level: L3
parent: L2-OBS-PACKAGES-b28b1ed9
title: packages/remote/src/shared/cloud module
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/remote/src/shared/cloud/__cloudE2eHarness.ts
  - packages/remote/src/shared/cloud/__faultRig.ts
  - packages/remote/src/shared/cloud/apiClient.ts
  - packages/remote/src/shared/cloud/auth.ts
  - packages/remote/src/shared/cloud/cloudHostBridge.ts
  - packages/remote/src/shared/cloud/cloudHostPaneSource.ts
  - packages/remote/src/shared/cloud/cloudTransportLimits.ts
  - packages/remote/src/shared/cloud/controllerCloudProvider.ts
  - packages/remote/src/shared/cloud/controllerIdentity.ts
  - packages/remote/src/shared/cloud/controllerInstanceId.ts
  - packages/remote/src/shared/cloud/deviceTrust.ts
  - packages/remote/src/shared/cloud/e2ee.ts
  - packages/remote/src/shared/cloud/keyBinding.ts
  - packages/remote/src/shared/cloud/priorityFrameQueue.ts
  - packages/remote/src/shared/cloud/remoteAllowlist.ts
  - packages/remote/src/shared/cloud/ridgeCloudProvider.ts
  - packages/remote/src/shared/cloud/workspaceScope.ts
public_interface:
  - "export async function activateKey(key: string, username?: string):
    Promise<CloudAuthState>"
  - "export async function activateThisDevice( deviceName: string, onProgress?:
    (p: DeviceActivationProgress)"
  - "export async function bootstrapFromCookie(): Promise<boolean>"
  - "export async function checkin(): Promise<CheckinResult>"
  - "export async function clearControllerIdentity(): Promise<void>"
  - export async function createControllerRig()
  - "export async function forgotPassword(email: string): Promise<void>"
  - "export async function getControllerPub(): Promise<Uint8Array>"
  - "export async function login(email: string, password: string):
    Promise<CloudAuthState>"
  - "export async function loginViaBrowser(opts: BrowserLoginOptions = {}):
    Promise<CloudAuthState>"
  - "export async function refreshMe(): Promise<CloudAuthState>"
  - "export async function resetPassword(email: string, code: string, password:
    string): Promise<CloudAuthState>"
  - "export async function runCloudDirChildrenE2E(opts: CloudE2eOptions):
    Promise<CloudE2eResult>"
  - "export async function signTrust(message: Uint8Array): Promise<Uint8Array>"
  - export class ApiError
  - export class AuthGatedTransport
  - export class CloudHostBridge
  - export class ControllerCloudProvider
  - export class E2eeSession
  - export class FaultDataChannel
  - export class FaultPeerConnection
  - export class FaultWebSocket
  - export class PriorityFrameQueue
  - export class RidgeCloudHost
  - "export function _resetCacheForTest(): void"
  - "export function _resetCliCacheForTest(): void"
  - "export function acceptWorkspaceShare(token: string, grantId: string):
    Promise<WorkspaceShareDto>"
  - "export function activateKey(token: string, key: string, username?: string):
    Promise<AuthResult>"
  - "export function authPoll(pollToken: string): Promise<AuthPollResult>"
  - "export function authRequest(client: 'desktop' | 'cli'):
    Promise<AuthRequestResult>"
  - "export function authorize(dc: FaultDataChannel, hostSession: E2eeSession,
    messageId: number): void"
  - "export function base64ToBytes(b64: string): Uint8Array"
  - "export function base64ToBytes(b64: string): Uint8Array | null"
  - "export function buildBindTranscript( hostEphPub: Uint8Array,
    controllerEphPub: Uint8Array, ): Uint8Array"
  - "export function buildIdBindContext( hostEphPub: Uint8Array,
    controllerEphPub: Uint8Array, deviceName: string, username: string, ):
    Uint8Array"
  - "export function buildNonce(dir: Direction, counter: bigint): Uint8Array"
  - "export function bytesToBase64(bytes: Uint8Array): string"
  - "export function checkOrPinDeviceIdentity( hostKey: string, idPub:
    Uint8Array, store: TrustStore = localStorageTrustStore()"
  - "export function checkin(token: string): Promise<CheckinResult>"
  - "export function clearDevicePin(hostKey: string, store: TrustStore =
    localStorageTrustStore()"
  - "export function cloudHttpScheme(domain: string, plaintext: boolean =
    DEV_PLAINTEXT): 'http' | 'https'"
  - "export function cloudWsScheme(domain: string, plaintext: boolean =
    DEV_PLAINTEXT): 'ws' | 'wss'"
  - "export function collectPaneIds(layout: unknown): Set<string>"
  - "export function completeE2ee( pc: FaultPeerConnection, ws: FaultWebSocket,
    ):"
  - "export function computeBindTag(totpCode: string, transcript: Uint8Array):
    Uint8Array"
  - "export function constantTimeEqual(a: Uint8Array, b: Uint8Array): boolean"
  - "export function createWorkspaceShare( token: string, input: { deviceName:
    string; workspaceId: string; grantee: string; role?: 'operator' }, ):
    Promise<WorkspaceShareDto>"
  - "export function decideKeyBinding( handshakePub: Uint8Array, signalingPub:
    Uint8Array | null, graceExpired: boolean, ): KeyBindingDecision"
  - "export function declineWorkspaceShare(token: string, grantId: string):
    Promise<WorkspaceShareDto>"
  - "export function decodeAnyHandshakeFrame(frame: Uint8Array): AnyHandshake"
  - "export function decodeHandshakeFrame(frame: Uint8Array): Uint8Array"
  - "export function decodeSignedHandshakeFrame(frame: Uint8Array):
    SignedHandshake"
  - "export function deleteDevice(token: string, name: string): Promise<"
  - "export function deriveSessionKey( myPrivateKey: Uint8Array, myPublicKey:
    Uint8Array, peerPublicKey: Uint8Array, ): Uint8Array"
  - "export function deviceActivate( token: string, pairingCode: string,
    deviceName: string, ): Promise<DeviceActivateResult>"
  - "export function deviceCode(): Promise<DeviceCodeResult>"
  - "export function devicePoll(pollToken: string): Promise<DevicePollResult>"
  - "export function encodeHandshakeFrame(publicKey: Uint8Array): Uint8Array"
  - "export function encodeSignedHandshakeFrame( ephPub: Uint8Array, idPub:
    Uint8Array, sig: Uint8Array, ): Uint8Array"
  - "export function filterWorkspaceResult( method: string, result: unknown,
    workspaceId: string, ): unknown"
  - "export function fingerprintOf(idPub: Uint8Array): string"
  - "export function forgotPassword(email: string): Promise<"
  - "export function generateEphemeralKeyPair(): EphemeralKeyPair"
  - "export function getIceServers(token: string): Promise<"
  - "export function getMe(token: string): Promise<"
  - "export function getOrCreateCli(): string"
  - "export function getPinnedFingerprint( hostKey: string, store: TrustStore =
    localStorageTrustStore()"
  - "export function getWorkspaceShareToken( token: string, grantId: string, ):
    Promise<WorkspaceShareToken>"
  - "export function hasActiveTime(state: CloudAuthState): boolean"
  - "export function hasCheckedInToday(state: CloudAuthState): boolean"
  - "export function installFaultGlobals(): void"
  - "export function isInsecureCloudDomain(domain: string): boolean"
  - "export function isLoggedIn(state: CloudAuthState): boolean"
  - "export function isMutatingMethod(method: string): boolean"
  - "export function isPremium(state: CloudAuthState): boolean"
  - "export function isRemoteAllowed(method: string): boolean"
  - "export function jsonrpcError( id: number | string, error: { code: number;
    message: string; data?: unknown }, ): Record<string, unknown>"
  - "export function jsonrpcResult(id: number | string, result: unknown):
    Record<string, unknown>"
  - "export function listDevices(token: string): Promise<"
  - "export function listSharedWithMe(token: string): Promise<"
  - "export function listWorkspaceShares(token: string): Promise<"
  - "export function localStorageTrustStore(): TrustStore"
  - "export function login(email: string, password: string): Promise<AuthResult>"
  - "export function logout(): void"
  - "export function makeCloudHostPaneSource(deps: CloudHostPaneSourceDeps):
    PaneOutputSource"
  - "export function negotiateHello(params: unknown): Record<string, unknown>"
  - "export function nonceCounter(nonce: Uint8Array): bigint"
  - "export function nonceDirection(nonce: Uint8Array): number"
  - "export function pathWithinRoots(path: string, roots: readonly string[]):
    boolean"
  - "export function planWorkspaceInvoke( method: string, rawParams: unknown,
    access: WorkspaceAccess, ): WorkspaceInvokePlan"
  - "export function publicEntryDomain(state: CloudAuthState): string | null"
  - "export function refreshAccess(): Promise<boolean>"
  - "export function register(email: string, password: string):
    Promise<AuthResult>"
  - "export function resetPassword(email: string, code: string, password:
    string): Promise<AuthResult>"
  - "export function revokeWorkspaceShare(token: string, grantId: string):
    Promise<WorkspaceShareDto>"
  - "export function session(): Promise<AuthResult>"
  - "export function setUnauthorizedHandler(fn: (()"
  - "export function setUsername(token: string, username: string): Promise<"
  - "export function snapshot(): CloudAuthState"
  - "export function toJsonRpcError(e: unknown):"
  - "export function verifyIdBindSignature( idPub: Uint8Array, context:
    Uint8Array, sig: Uint8Array, ): boolean"
  - "export function verifyWorkspaceShareAccess( deviceToken: string, token:
    string, ): Promise<WorkspaceShareScope>"
  - export interface AuthRequestResult
  - export interface AuthResult
  - export interface BrowserLoginOptions
  - export interface BrowserLoginProgress
  - export interface ChannelBackpressure
  - export interface CheckinResult
  - export interface CloudAuthState
  - export interface CloudControllerSession
  - export interface CloudE2eOptions
  - export interface CloudE2eProbe
  - export interface CloudE2eResult
  - export interface CloudHostBridgeConfig
  - export interface CloudHostBridgeLike
  - export interface CloudHostPaneSourceDeps
  - export interface ControllerCloudProviderConfig
  - export interface DeviceActivateResult
  - export interface DeviceActivationProgress
  - export interface DeviceCodeResult
  - export interface DeviceDto
  - export interface EphemeralKeyPair
  - export interface IceServer
  - export interface PriorityFrameQueueOptions
  - export interface RidgeCloudHostCallbacks
  - export interface RidgeCloudHostConfig
  - export interface SignedHandshake
  - export interface TrustStore
  - export interface UserDto
  - export interface WorkspaceAccess
  - export interface WorkspaceScopeAssertion
  - export interface WorkspaceShareDto
  - export interface WorkspaceShareScope
  - export interface WorkspaceShareToken
  - export type AnyHandshake
  - export type ApiErrorCode
  - export type AuthPollResult
  - export type DevicePollResult
  - export type Direction
  - export type HostSignalState
  - export type InvokeFn
  - export type KeyBindingDecision
  - export type KeyBindingMode
  - export type ListenFn
  - export type PaneOutputSource
  - export type SendFrameFn
  - export type TofuResult
  - export type TotpBindVerifier
  - export type TotpVerifier
  - export type Unsubscribe
  - export type WorkspaceInvokePlan
---

# packages/remote/src/shared/cloud module

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
