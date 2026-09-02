---
id: L4-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-E2EE-TS-93dc9184
level: L4
parent: L3-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-52c9de8f
title: e2ee.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/remote/src/shared/cloud/e2ee.ts
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
  - export class E2eeSession
  - "export function base64ToBytes(b64: string): Uint8Array | null"
  - "export function buildBindTranscript( hostEphPub: Uint8Array,
    controllerEphPub: Uint8Array, ): Uint8Array"
  - "export function buildIdBindContext( hostEphPub: Uint8Array,
    controllerEphPub: Uint8Array, deviceName: string, username: string, ):
    Uint8Array"
  - "export function buildNonce(dir: Direction, counter: bigint): Uint8Array"
  - "export function bytesToBase64(bytes: Uint8Array): string"
  - "export function computeBindTag(totpCode: string, transcript: Uint8Array):
    Uint8Array"
  - "export function decodeAnyHandshakeFrame(frame: Uint8Array): AnyHandshake"
  - "export function decodeHandshakeFrame(frame: Uint8Array): Uint8Array"
  - "export function decodeSignedHandshakeFrame(frame: Uint8Array):
    SignedHandshake"
  - "export function deriveSessionKey( myPrivateKey: Uint8Array, myPublicKey:
    Uint8Array, peerPublicKey: Uint8Array, ): Uint8Array"
  - "export function encodeHandshakeFrame(publicKey: Uint8Array): Uint8Array"
  - "export function encodeSignedHandshakeFrame( ephPub: Uint8Array, idPub:
    Uint8Array, sig: Uint8Array, ): Uint8Array"
  - "export function generateEphemeralKeyPair(): EphemeralKeyPair"
  - "export function nonceCounter(nonce: Uint8Array): bigint"
  - "export function nonceDirection(nonce: Uint8Array): number"
  - "export function verifyIdBindSignature( idPub: Uint8Array, context:
    Uint8Array, sig: Uint8Array, ): boolean"
  - export interface EphemeralKeyPair
  - export interface SignedHandshake
  - export type AnyHandshake
  - export type Direction
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

# e2ee.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
