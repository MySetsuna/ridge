---
id: L4-OBS-SRC-SHARED-SIDEBAR-TYPES-TS-7f5c67d5
level: L4
parent: L3-OBS-SRC-SHARED-SIDEBAR-6574e78e
title: types.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/shared/sidebar/types.ts
test_targets:
  - src/shared/sidebar/gitRequestLifecycle.test.ts
  - src/shared/sidebar/sidebarRequestLifecycle.test.ts
public_interface:
  - export interface DirListing
  - export interface FileEntry
  - export interface GitCommit
  - export interface GitDiffFile
  - export interface GitGraph
  - export interface GitInfo
  - export interface SearchHit
  - export interface SidebarProvider
verified_by:
  - TEST-OBS-SRC-SHARED-SIDEBAR-GITREQUESTLIFECYCLE-TEST-TS-6975b120
  - TEST-OBS-SRC-SHARED-SIDEBAR-SIDEBARREQUESTLIFECYCLE-TEST-TS-1c627569
---

# types.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
