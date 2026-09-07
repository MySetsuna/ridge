---
id: L2-REMOTE-MOBILE-SEMANTIC-V2
level: L2
title: Remote mobile semantic terminal v2 and workspace-scoped state
status: LOCKED
lifecycle: ACTIVE
parent: L1-PROJECT-001
code_targets:
  - Cargo.toml
  - Cargo.lock
  - package.json
  - packages/ridge-term/**
  - packages/ridge-kernel/**
  - packages/ridge-cli/**
  - packages/remote/**
  - src-tauri/**
  - src/remote/**
  - scripts/sync-signaling.mjs
  - archive/remote-v1/**
test_targets:
  - packages/ridge-term/**
  - packages/ridge-kernel/**
  - packages/ridge-cli/**
  - packages/remote/**
  - src-tauri/**
  - src/remote/**
  - scripts/**/*.test.*
  - tests/**
---

# Remote mobile semantic terminal v2 and workspace-scoped state

Ridge Remote mobile shall address terminal switching, rendering freshness, TUI mouse capture, and workspace isolation as one semantic protocol change. Every terminal identity and operation uses the complete workspaceId plus paneId reference. LAN, Cloud, and rdg negotiate terminal protocol v2 with no raw-v1 fallback; a version mismatch is rejected with an explicit upgrade-required reason. The host parses PTY output and sends one exact semantic snapshot followed by contiguous revisioned semantic deltas for the single active remote pane; inactive panes retain only bounded session-local render caches and receive no background terminal stream. Snapshot state covers both primary and alternate screens, visible and recent history, cursors and saved cursors, scroll regions, rendering/input modes including mouse reporting, title, cwd, graphemes, attributes, wrapping, and hyperlink spans. Snapshot capture and registration of later deltas form one ordered transaction; duplicate frames are ignored and gaps trigger one coalesced snapshot refresh. Remote input, pointer, resize, history, activation, and deactivation are bound to a workspace-scoped pane activation token so late frames and input cannot cross pane transitions. Mobile navigation owns one atomic PaneRef selection with generation fencing. Workspace and per-workspace pane lists use host-scoped TanStack Query keys, show session cache immediately, refresh silently, and update from scoped topology events without independent polling or transient peek stores. Only lightweight navigation preferences persist across reload and their keys are scoped by authenticated host identity; terminal and Query payloads are not persisted. Normal touch forwards structured pointer gestures to mouse-reporting TUIs, while explicit selection mode always performs local selection/copy and sends no pointer input. Cancel, switch, disconnect, and blur settle an active pointer gesture without duplicate releases. Obsolete raw mobile feed/replay modules are archived rather than deleted. Acceptance requires protocol golden conformance, exact snapshot/delta parity, workspace-collision and rapid-switch tests, active-only streaming, LAN/Cloud/rdg transport parity, real mouse byte assertions, local-selection override, weak-network recovery, synchronized version rejection, remote mobile build, repository tests, type/lint checks, stc verify, and stc complete.
