---
id: L3-OBS-SRC-LIB-REMOTE-18f2f4d2
level: L3
parent: L2-OBS-SRC-25a66342
title: src/lib/remote module
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/remote/MinimizeButton.svelte
  - src/lib/remote/qrcode.d.ts
  - src/lib/remote/QrCode.svelte
  - src/lib/remote/remoteBootMode.ts
  - src/lib/remote/RemotePanel.svelte
  - src/lib/remote/totpIdentitySync.ts
  - src/lib/remote/TreeNodeRow.svelte
public_interface:
  - "export function remoteBootMode( hostname: string, search: string,
    baseDomain: string, ): RemoteBootMode"
  - "export function startTotpIdentitySync(invoke: InvokeFn, store: StoreLike):
    () => void"
  - "export function toCanvas( canvas: HTMLCanvasElement, text: string,
    options?: { width?: number; margin?: number; color?: { dark?: string;
    light?: string }; }, cb?: (error: Error | null)"
  - export interface TreeNode
  - export type RemoteBootMode
---

# src/lib/remote module

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
