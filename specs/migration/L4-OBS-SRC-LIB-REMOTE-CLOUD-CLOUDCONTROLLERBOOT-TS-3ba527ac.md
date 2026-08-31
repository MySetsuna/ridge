---
id: L4-OBS-SRC-LIB-REMOTE-CLOUD-CLOUDCONTROLLERBOOT-TS-3ba527ac
level: L4
parent: L3-OBS-SRC-LIB-REMOTE-CLOUD-643692b0
title: cloudControllerBoot.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/remote/cloud/cloudControllerBoot.ts
test_targets:
  - src/lib/remote/cloud/cloudControllerBoot.integration.test.ts
  - src/lib/remote/cloud/cloudControllerBoot.test.ts
  - src/lib/remote/cloud/cloudHostStore.test.ts
  - src/lib/remote/cloud/cloudHostTopologyLink.test.ts
  - src/lib/remote/cloud/sharedWorkspaceProjection.behavior.test.ts
  - src/lib/remote/cloud/sharedWorkspaceProjection.test.ts
public_interface:
  - "export async function performTrustHandshake( adapter: CloudWebrtcAdapter,
    timeoutMs = TRUST_GRANT_TIMEOUT_MS, ): Promise<boolean>"
  - "export function activeCloudController(): CloudControllerHandle | null"
  - "export function bootCloudControllerFromUrl( search: string, ui?:
    Pick<CloudControllerBootParams, 'onState' | 'onError'>, hostname?: string,
    ): CloudControllerHandle | null"
  - "export function parseCloudControllerHostname( hostname: string, ):"
  - "export function parseCloudControllerUrl( search: string, ):"
  - "export function startCloudControllerBoot( params:
    CloudControllerBootParams, options: CloudControllerBootOptions = {}, ):
    CloudControllerHandle"
  - "export function verifyTotpOverControl( adapter: CloudWebrtcAdapter, code:
    string, timeoutMs = TOTP_VERIFY_TIMEOUT_MS, ): Promise<boolean>"
  - export interface CloudControllerBootOptions
  - export interface CloudControllerBootParams
  - export interface CloudControllerHandle
verified_by:
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-CLOUDCONTROLLERBOOT-INTEGRATION-TEST-TS-d930e02e
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-CLOUDCONTROLLERBOOT-TEST-TS-7f1c661a
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-CLOUDHOSTSTORE-TEST-TS-a2df9830
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-CLOUDHOSTTOPOLOGYLINK-TEST-TS-7c92f09e
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-SHAREDWORKSPACEPROJECTION-BEHAVIOR-TEST-TS-69e303c4
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-SHAREDWORKSPACEPROJECTION-TEST-TS-0dde9c6f
---

# cloudControllerBoot.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
