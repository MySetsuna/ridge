---
id: L4-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-APICLIENT-TS-3a5d37d5
level: L4
parent: L3-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-52c9de8f
title: apiClient.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/remote/src/shared/cloud/apiClient.ts
test_targets:
  - packages/remote/src/shared/cloud/__cloudE2eHarness.test.ts
  - packages/remote/src/shared/cloud/apiClient.refresh.test.ts
  - packages/remote/src/shared/cloud/apiClient.test.ts
  - packages/remote/src/shared/cloud/auth.test.ts
  - packages/remote/src/shared/cloud/cloudHostBridge.test.ts
  - packages/remote/src/shared/cloud/cloudHostPaneSource.test.ts
  - packages/remote/src/shared/cloud/controllerCloudProvider.test.ts
  - packages/remote/src/shared/cloud/controllerIdentity.test.ts
  - packages/remote/src/shared/cloud/controllerInstanceId.test.ts
  - packages/remote/src/shared/cloud/deviceTrust.test.ts
  - packages/remote/src/shared/cloud/e2ee.test.ts
  - packages/remote/src/shared/cloud/faultInjection.test.ts
  - packages/remote/src/shared/cloud/keyBinding.test.ts
  - packages/remote/src/shared/cloud/priorityFrameQueue.test.ts
  - packages/remote/src/shared/cloud/remoteAllowlist.test.ts
  - packages/remote/src/shared/cloud/ridgeCloudProvider.test.ts
  - packages/remote/src/shared/cloud/signaling/conformance.test.ts
  - packages/remote/src/shared/cloud/signaling/drift.test.ts
  - packages/remote/src/shared/cloud/weakNetLab.test.ts
  - packages/remote/src/shared/cloud/workspaceScope.test.ts
public_interface:
  - export class ApiError
  - "export function acceptWorkspaceShare(token: string, grantId: string):
    Promise<WorkspaceShareDto>"
  - "export function activateKey(token: string, key: string, username?: string):
    Promise<AuthResult>"
  - "export function authPoll(pollToken: string): Promise<AuthPollResult>"
  - "export function authRequest(client: 'desktop' | 'cli'):
    Promise<AuthRequestResult>"
  - "export function checkin(token: string): Promise<CheckinResult>"
  - "export function cloudHttpScheme(domain: string, plaintext: boolean =
    DEV_PLAINTEXT): 'http' | 'https'"
  - "export function cloudWsScheme(domain: string, plaintext: boolean =
    DEV_PLAINTEXT): 'ws' | 'wss'"
  - "export function createWorkspaceShare( token: string, input: { deviceName:
    string; workspaceId: string; grantee: string; role?: 'operator' }, ):
    Promise<WorkspaceShareDto>"
  - "export function declineWorkspaceShare(token: string, grantId: string):
    Promise<WorkspaceShareDto>"
  - "export function deleteDevice(token: string, name: string): Promise<"
  - "export function deviceActivate( token: string, pairingCode: string,
    deviceName: string, ): Promise<DeviceActivateResult>"
  - "export function deviceCode(): Promise<DeviceCodeResult>"
  - "export function devicePoll(pollToken: string): Promise<DevicePollResult>"
  - "export function forgotPassword(email: string): Promise<"
  - "export function getIceServers(token: string): Promise<"
  - "export function getMe(token: string): Promise<"
  - "export function getWorkspaceShareToken( token: string, grantId: string, ):
    Promise<WorkspaceShareToken>"
  - "export function isInsecureCloudDomain(domain: string): boolean"
  - "export function listDevices(token: string): Promise<"
  - "export function listSharedWithMe(token: string): Promise<"
  - "export function listWorkspaceShares(token: string): Promise<"
  - "export function login(email: string, password: string): Promise<AuthResult>"
  - "export function register(email: string, password: string):
    Promise<AuthResult>"
  - "export function resetPassword(email: string, code: string, password:
    string): Promise<AuthResult>"
  - "export function revokeWorkspaceShare(token: string, grantId: string):
    Promise<WorkspaceShareDto>"
  - "export function session(): Promise<AuthResult>"
  - "export function setUnauthorizedHandler(fn: (()"
  - "export function setUsername(token: string, username: string): Promise<"
  - "export function verifyWorkspaceShareAccess( deviceToken: string, token:
    string, ): Promise<WorkspaceShareScope>"
  - export interface AuthRequestResult
  - export interface AuthResult
  - export interface CheckinResult
  - export interface DeviceActivateResult
  - export interface DeviceCodeResult
  - export interface DeviceDto
  - export interface IceServer
  - export interface UserDto
  - export interface WorkspaceShareDto
  - export interface WorkspaceShareScope
  - export interface WorkspaceShareToken
  - export type ApiErrorCode
  - export type AuthPollResult
  - export type DevicePollResult
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-CLOUDE2EHARNESS-TEST-TS-369a5af1
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-APICLIENT-REFRESH-TEST-TS-ae2beee9
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-APICLIENT-TEST-TS-54ddea70
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-AUTH-TEST-TS-8907fcc2
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-CLOUDHOSTBRIDGE-TEST-TS-731be498
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-CLOUDHOSTPANESOURCE-TEST-TS-5698aba6
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-CONTROLLERCLOUDPROVIDER-TEST-TS-fa839b23
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-CONTROLLERIDENTITY-TEST-TS-dd543ad2
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-CONTROLLERINSTANCEID-TEST-TS-471305fa
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-DEVICETRUST-TEST-TS-356a0744
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-E2EE-TEST-TS-81cb1212
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-FAULTINJECTION-TEST-TS-7005e479
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-KEYBINDING-TEST-TS-3c0d052f
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-PRIORITYFRAMEQUEUE-TEST-TS-8e7fa935
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-REMOTEALLOWLIST-TEST-TS-ad14903f
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-RIDGECLOUDPROVIDER-TEST-TS-149b31d2
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-SIGNALING-CONFORMANCE-TEST-TS-50aa4f26
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-SIGNALING-DRIFT-TEST-TS-52f30651
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-WEAKNETLAB-TEST-TS-e7298069
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-WORKSPACESCOPE-TEST-TS-184beaf5
---

# apiClient.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
