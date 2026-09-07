---
id: L2-AGENT-COMMUNE-EXACT-BINDING
level: L2
title: Agent Commune exact workspace conversation binding
status: LOCKED
lifecycle: ACTIVE
parent: L1-PROJECT-001
code_targets:
  - src-tauri/src/commands/project.rs
  - src-tauri/src/commands/teammate.rs
  - src-tauri/src/teammate/**
  - src-tauri/src/remote_host_impl.rs
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

# Agent Commune exact workspace conversation binding

Agent's Commune shall bind every member, group, transcript history, resume action, and delivery target to a single exact workspace-scoped conversation identity. Provider-specific transcript indexing shall expose an opaque binding reference rather than infer identity from agent names or current working directories. Each member has a stable member identity distinct from its delivery identity and reports bound, unbound, or ambiguous state; only an explicit exact transcript binding may resolve unbound or ambiguous candidates. History and recent replies are queried, cached, and rendered per workspace and binding revision, never host-wide, and resume uses the exact bound transcript reference. Individual and group messages shall go exclusively through the built-in teammate service and durable Hub queue, with receipt status; PTY input is never a fallback delivery path. Auto-discovered members shall receive service-resolvable delivery identities without altering member identity. Group and teammate-setting state is backend canonical, revisioned, workspace scoped, and migrates legacy group membership only on an exact member identity match; unresolved legacy entries are archived with a visible recovery path. Desktop and Remote/mobile clients shall share these isolation and delivery guarantees. Acceptance requires provider locator tests, workspace collision tests, no CWD/name fallback tests, explicit binding tests, exact legacy migration and archive tests, service-only delivery tests with durable receipt status, remote workspace-query/cache tests, and the repository test, lint, typecheck, SpecTree verification and completion gates.
