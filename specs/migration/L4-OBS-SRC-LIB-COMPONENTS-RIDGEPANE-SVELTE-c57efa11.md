---
id: L4-OBS-SRC-LIB-COMPONENTS-RIDGEPANE-SVELTE-c57efa11
level: L4
parent: L3-OBS-SRC-LIB-COMPONENTS-28f39e09
title: RidgePane.svelte
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/components/RidgePane.svelte
test_targets:
  - src/lib/components/customTheme.test.ts
  - src/lib/components/explorerPaste.test.ts
  - src/lib/components/GitGraph.test.ts
  - src/lib/components/inputBufferTracker.test.ts
  - src/lib/components/interactionContracts.test.ts
  - src/lib/components/SettingsPanel.test.ts
  - src/lib/components/SidebarLazyMount.test.ts
  - src/lib/components/SplitContainer.test.ts
verified_by:
  - TEST-OBS-SRC-LIB-COMPONENTS-CUSTOMTHEME-TEST-TS-0b73284d
  - TEST-OBS-SRC-LIB-COMPONENTS-EXPLORERPASTE-TEST-TS-e29ef2f9
  - TEST-OBS-SRC-LIB-COMPONENTS-GITGRAPH-TEST-TS-69aabbd2
  - TEST-OBS-SRC-LIB-COMPONENTS-INPUTBUFFERTRACKER-TEST-TS-a37220a2
  - TEST-OBS-SRC-LIB-COMPONENTS-INTERACTIONCONTRACTS-TEST-TS-6ff9d90a
  - TEST-OBS-SRC-LIB-COMPONENTS-SETTINGSPANEL-TEST-TS-a77c4772
  - TEST-OBS-SRC-LIB-COMPONENTS-SIDEBARLAZYMOUNT-TEST-TS-a0bebf5d
  - TEST-OBS-SRC-LIB-COMPONENTS-SPLITCONTAINER-TEST-TS-1932765d
---

# RidgePane.svelte

A desktop context menu opened while an inline TUI signal is live grants that pane a bounded host-interaction lease. Choosing a non-terminal-replacing menu action renews the lease and restores pane focus, so a visible TUI cursor or heuristic decay during menu use cannot route subsequent input through shell mode. Plain shell context menus never create the lease.
