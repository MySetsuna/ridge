---
id: L3-OBS-SRC-LIB-ACTIONS-531bf1fb
level: L3
parent: L2-OBS-SRC-25a66342
title: src/lib/actions module
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/actions/autoGrow.ts
  - src/lib/actions/hostSessionDrag.ts
  - src/lib/actions/overlayScroll.ts
  - src/lib/actions/paneDockDrag.ts
  - src/lib/actions/portal.ts
public_interface:
  - "export function autoGrow(node: HTMLTextAreaElement, opts: AutoGrowOpts =
    {})"
  - "export function hostAttachRequestAt( params: HostSessionDragParams,
    targetPaneId: string, region: AttachRegion, ): HostAttachRequest"
  - "export function hostSessionDrag(node: HTMLElement, params:
    HostSessionDragParams)"
  - "export function overlayScroll( node: HTMLElement, params:
    OverlayScrollOptions | undefined = undefined )"
  - "export function paneDockDrag(node: HTMLElement, params: Params)"
  - export interface AutoGrowOpts
  - export interface HostSessionDragParams
  - export interface OverlayScrollLayout
  - export interface OverlayScrollOptions
  - export interface PortalOptions
  - export type OverlayScrollPreset
---

# src/lib/actions module

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
