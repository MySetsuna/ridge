---
id: L4-OBS-SRC-TAURI-SRC-COMMANDS-PANE-RS-083d2191
level: L4
parent: L3-OBS-SRC-TAURI-SRC-COMMANDS-7ed73efa
title: pane.rs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src-tauri/src/commands/pane.rs
test_targets:
  - packages/remote/src/shared/cloud/cloudHostPaneSource.test.ts
  - packages/remote/src/shared/hosts/foreignPaneStatus.test.ts
  - packages/remote/src/shared/terminal/initialPaneFit.test.ts
  - packages/remote/src/shared/terminal/paneDockResolve.test.ts
  - packages/remote/src/shared/terminal/paneGeometry.test.ts
  - packages/remote/src/shared/terminal/paneInputGate.test.ts
  - packages/remote/src/shared/terminal/paneOrigin.test.ts
  - packages/remote/src/shared/terminal/paneShell.test.ts
  - packages/remote/src/shared/transport/paneRpcScheduler.test.ts
  - src/lib/actions/paneDockDrag.test.ts
  - src/lib/components/SettingsPanel.test.ts
  - src/lib/hosts/remotePaneBindings.test.ts
  - src/lib/stores/paneGitStatus.test.ts
  - src/lib/stores/paneTree.coverage.test.ts
  - src/lib/stores/paneTree.test.ts
  - src/lib/teammate/agentPaneHighlightSync.test.ts
  - src/lib/teammate/hitlAuditPanel.test.ts
  - src/lib/terminal/desktopPaneResize.test.ts
  - src/lib/terminal/paneSizeSync.test.ts
  - src/remote/lib/paneFeedScheduler.test.ts
  - src/remote/lib/paneLifecycle.test.ts
  - src/remote/lib/paneSwitchBuffer.test.ts
  - src/remote/lib/RemoteGitPanel.test.ts
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-CLOUDHOSTPANESOURCE-TEST-TS-5698aba6
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-HOSTS-FOREIGNPANESTATUS-TEST-TS-9fbe3344
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-INITIALPANEFIT-TEST-TS-fe5c49cf
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-PANEDOCKRESOLVE-TEST-TS-4fdcb558
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-PANEGEOMETRY-TEST-TS-ad29e3d7
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-PANEINPUTGATE-TEST-TS-06a1530b
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-PANEORIGIN-TEST-TS-6d99d562
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-PANESHELL-TEST-TS-e9209065
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-PANERPCSCHEDULER-TEST-TS-ad772920
  - TEST-OBS-SRC-LIB-ACTIONS-PANEDOCKDRAG-TEST-TS-3c0ebf73
  - TEST-OBS-SRC-LIB-COMPONENTS-SETTINGSPANEL-TEST-TS-a77c4772
  - TEST-OBS-SRC-LIB-HOSTS-REMOTEPANEBINDINGS-TEST-TS-2377e089
  - TEST-OBS-SRC-LIB-STORES-PANEGITSTATUS-TEST-TS-73bc40e2
  - TEST-OBS-SRC-LIB-STORES-PANETREE-COVERAGE-TEST-TS-747ca055
  - TEST-OBS-SRC-LIB-STORES-PANETREE-TEST-TS-8fb828ce
  - TEST-OBS-SRC-LIB-TEAMMATE-AGENTPANEHIGHLIGHTSYNC-TEST-TS-68715b97
  - TEST-OBS-SRC-LIB-TEAMMATE-HITLAUDITPANEL-TEST-TS-44e3a8b8
  - TEST-OBS-SRC-LIB-TERMINAL-DESKTOPPANERESIZE-TEST-TS-38627314
  - TEST-OBS-SRC-LIB-TERMINAL-PANESIZESYNC-TEST-TS-8a1e46ba
  - TEST-OBS-SRC-REMOTE-LIB-PANEFEEDSCHEDULER-TEST-TS-6040ce95
  - TEST-OBS-SRC-REMOTE-LIB-PANELIFECYCLE-TEST-TS-b323d4f0
  - TEST-OBS-SRC-REMOTE-LIB-PANESWITCHBUFFER-TEST-TS-8eee2043
  - TEST-OBS-SRC-REMOTE-LIB-REMOTEGITPANEL-TEST-TS-40e79e76
---

# pane.rs

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
