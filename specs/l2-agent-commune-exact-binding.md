---
id: L2-AGENT-COMMUNE-EXACT-BINDING
level: L2
title: Agent Commune exact workspace conversation binding
status: LOCKED
lifecycle: ACTIVE
parent: L1-PROJECT-001
code_targets:
  - src-tauri/src/commands/project.rs
  - src-tauri/src/teammate/**
  - src/lib/teammate/**
  - src/lib/components/SettingsPanel.svelte
  - src/remote/lib/SidebarTeamRoster.svelte
  - src/remote/lib/cloudRemote.ts
  - src/remote/lib/remoteQueries.ts
  - packages/remote/src/**
  - packages/ridge-core/src/capability.rs
  - packages/ridge-mcp/src/server.rs
  - archive/agent-commune-v1/**
test_targets:
  - src-tauri/src/**
  - src/lib/teammate/**/*.test.*
  - src/remote/**/*.test.*
  - packages/remote/src/**/*.test.*
  - packages/ridge-mcp/src/**
  - packages/ridge-core/src/**
---

# Agent Commune exact conversation binding and global presentation

Commune aggregates all Agents and protocol history available to the authenticated Host connection, independent of the active terminal workspace. Workspace remains an internal authorization and source identity: every member, group record, transcript row, approval and delivery action retains its exact original workspace/pane/member/session and binding revision. Aggregation never falls back to names or CWD, crosses ungranted workspaces, merges permissions, or treats a transport receipt as Agent task completion. Existing groups are presented together while mutations route to their original service identity.

Members have stable identities distinct from delivery identities; ambiguous/unbound protocol sessions stay explicit. History and replies remain cached per source workspace and binding revision, then merged into the global view. Individual and group protocol messages use the durable Hub; explicit user terminal actions use the existing fenced manual input path. Remote and desktop share these source-isolation guarantees. Transient reads retain confirmed data with visible stale/error state; removed sources and superseded reads cannot repopulate the current projection.

Acceptance covers concurrent same-name Agents across multiple workspaces, global discovery/replies/history while switching terminal workspaces, exact target sends and approvals, isolated pagination and failures, preserved group records, and no name/CWD fallback or automatic input retry. Run real Kernel protocol/UI integration plus repository test/lint/typecheck and SpecTree verify/complete.
