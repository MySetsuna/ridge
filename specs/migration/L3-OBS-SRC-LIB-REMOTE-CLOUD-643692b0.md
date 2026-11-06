---
id: L3-OBS-SRC-LIB-REMOTE-CLOUD-643692b0
level: L3
parent: L2-OBS-SRC-25a66342
title: src/lib/remote/cloud module
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/remote/cloud/CheckinGateCard.svelte
  - src/lib/remote/cloud/cloudControllerBoot.ts
  - src/lib/remote/cloud/cloudHostStore.ts
  - src/lib/remote/cloud/CloudProModal.svelte
  - src/lib/remote/cloud/sharedWorkspaceProjection.ts
public_interface:
  - "export async function connectCloudHostTopologyLink( hostDevice: string,
    totp: string, ): Promise<HostTopologyLink>"
  - "export async function goOffline(): Promise<void>"
  - "export async function goOnline(): Promise<void>"
  - "export async function openSharedWorkspaceProjection(input: { grantId:
    string; workspaceId: string; name: string; ownerUsername: string;
    deviceName: string; }): Promise<void>"
  - "export async function performTrustHandshake( adapter: CloudWebrtcAdapter,
    timeoutMs = TRUST_GRANT_TIMEOUT_MS, ): Promise<boolean>"
  - export class CloudHostTopologyLink
  - "export function activeCloudController(): CloudControllerHandle | null"
  - "export function assertShareTokenScope( input: { grantId: string;
    workspaceId: string; deviceName: string }, scoped: { grantId: string;
    workspaceId: string; deviceName: string; delegable: boolean }, ): void"
  - "export function blacklistController(cid: string): void"
  - "export function bootCloudControllerFromUrl( search: string, ui?:
    Pick<CloudControllerBootParams, 'onState' | 'onError'>, hostname?: string,
    ): CloudControllerHandle | null"
  - "export function closeSharedWorkspaceProjection(): void"
  - "export function currentSharedWorkspaceProjection():
    SharedWorkspaceProjection | null"
  - "export function isHostOnline(): boolean"
  - "export function kickController(cid: string): void"
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
  - export interface SharedWorkspaceProjection
test_targets: []
---

# src/lib/remote/cloud module

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.

## CHG-090 映射处置

保留仍存在的观察目标。移除失效的现役映射，已迁出路径留历史证据，不能据观察记录推断新位置已实现同一行为。

`src/lib/remote/cloud/cloudHostTopologyLink.ts` → `docs/archive/CHG-070/frontend-cloud-topology/cloudHostTopologyLink.ts`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob fe7afc793d3a41e2200c160861ed33f9e0306a0a

原始映射保留如下，历史归档仅用于溯源。

```json
{
  "codeTargets": [
    "src/lib/remote/cloud/CheckinGateCard.svelte",
    "src/lib/remote/cloud/cloudControllerBoot.ts",
    "src/lib/remote/cloud/cloudHostStore.ts",
    "src/lib/remote/cloud/cloudHostTopologyLink.ts",
    "src/lib/remote/cloud/CloudProModal.svelte",
    "src/lib/remote/cloud/sharedWorkspaceProjection.ts"
  ],
  "testTargets": []
}
```
