---
id: L4-OBS-SRC-LIB-REMOTE-CLOUD-SHAREDWORKSPACEPROJECTION-TS-27c07ba9
level: L4
parent: L3-OBS-SRC-LIB-REMOTE-CLOUD-643692b0
title: sharedWorkspaceProjection.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/remote/cloud/sharedWorkspaceProjection.ts
test_targets:
  - src/lib/remote/cloud/cloudControllerBoot.integration.test.ts
  - src/lib/remote/cloud/cloudControllerBoot.test.ts
  - src/lib/remote/cloud/cloudHostStore.test.ts
  - src/lib/remote/cloud/cloudHostTopologyLink.test.ts
  - src/lib/remote/cloud/sharedWorkspaceProjection.behavior.test.ts
  - src/lib/remote/cloud/sharedWorkspaceProjection.test.ts
public_interface:
  - "export async function openSharedWorkspaceProjection(input: { grantId:
    string; workspaceId: string; name: string; ownerUsername: string;
    deviceName: string; }): Promise<void>"
  - "export function assertShareTokenScope( input: { grantId: string;
    workspaceId: string; deviceName: string }, scoped: { grantId: string;
    workspaceId: string; deviceName: string; delegable: boolean }, ): void"
  - "export function closeSharedWorkspaceProjection(): void"
  - "export function currentSharedWorkspaceProjection():
    SharedWorkspaceProjection | null"
  - export interface SharedWorkspaceProjection
verified_by:
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-CLOUDCONTROLLERBOOT-INTEGRATION-TEST-TS-d930e02e
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-CLOUDCONTROLLERBOOT-TEST-TS-7f1c661a
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-CLOUDHOSTSTORE-TEST-TS-a2df9830
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-CLOUDHOSTTOPOLOGYLINK-TEST-TS-7c92f09e
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-SHAREDWORKSPACEPROJECTION-BEHAVIOR-TEST-TS-69e303c4
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-SHAREDWORKSPACEPROJECTION-TEST-TS-0dde9c6f
---

# sharedWorkspaceProjection.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
