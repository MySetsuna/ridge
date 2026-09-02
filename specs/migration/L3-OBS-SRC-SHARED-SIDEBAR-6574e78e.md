---
id: L3-OBS-SRC-SHARED-SIDEBAR-6574e78e
level: L3
parent: L2-OBS-SRC-25a66342
title: src/shared/sidebar module
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/shared/sidebar/SidebarFileTree.svelte
  - src/shared/sidebar/SidebarGitPanel.svelte
  - src/shared/sidebar/SidebarSearch.svelte
  - src/shared/sidebar/types.ts
public_interface:
  - export interface DirListing
  - export interface FileEntry
  - export interface GitCommit
  - export interface GitDiffFile
  - export interface GitGraph
  - export interface GitInfo
  - export interface SearchHit
  - export interface SidebarProvider
---

# src/shared/sidebar module

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
