---
id: L4-OBS-SRC-LIB-REMOTE-CLOUD-CLOUDHOSTSTORE-TS-86f442a0
level: L4
parent: L3-OBS-SRC-LIB-REMOTE-CLOUD-643692b0
title: cloudHostStore.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/remote/cloud/cloudHostStore.ts
test_targets:
  - src/lib/remote/cloud/cloudControllerBoot.integration.test.ts
  - src/lib/remote/cloud/cloudControllerBoot.test.ts
  - src/lib/remote/cloud/cloudHostStore.test.ts
  - src/lib/remote/cloud/cloudHostTopologyLink.test.ts
  - src/lib/remote/cloud/sharedWorkspaceProjection.behavior.test.ts
  - src/lib/remote/cloud/sharedWorkspaceProjection.test.ts
public_interface:
  - "export async function goOffline(): Promise<void>"
  - "export async function goOnline(): Promise<void>"
  - "export function blacklistController(cid: string): void"
  - "export function isHostOnline(): boolean"
  - "export function kickController(cid: string): void"
verified_by:
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-CLOUDCONTROLLERBOOT-INTEGRATION-TEST-TS-d930e02e
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-CLOUDCONTROLLERBOOT-TEST-TS-7f1c661a
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-CLOUDHOSTSTORE-TEST-TS-a2df9830
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-CLOUDHOSTTOPOLOGYLINK-TEST-TS-7c92f09e
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-SHAREDWORKSPACEPROJECTION-BEHAVIOR-TEST-TS-69e303c4
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-SHAREDWORKSPACEPROJECTION-TEST-TS-0dde9c6f
---

# cloudHostStore.ts

Cloud Host 接收 Remote 內核定義的平鋪 RPC 參數；桌面 WebView 經單一 dispatch_remote_invoke(method, args) shell 端口接回既有 remote dispatcher。unscoped 與 workspace-scoped 路徑一致，不引入 Tauri command 專用參數形狀。
