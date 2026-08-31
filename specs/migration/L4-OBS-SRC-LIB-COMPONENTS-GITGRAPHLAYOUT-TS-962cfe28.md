---
id: L4-OBS-SRC-LIB-COMPONENTS-GITGRAPHLAYOUT-TS-962cfe28
level: L4
parent: L3-OBS-SRC-LIB-COMPONENTS-28f39e09
title: gitGraphLayout.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/components/gitGraphLayout.ts
test_targets:
  - src/lib/components/customTheme.test.ts
  - src/lib/components/explorerPaste.test.ts
  - src/lib/components/GitGraph.test.ts
  - src/lib/components/inputBufferTracker.test.ts
  - src/lib/components/interactionContracts.test.ts
  - src/lib/components/SettingsPanel.test.ts
  - src/lib/components/SidebarLazyMount.test.ts
  - src/lib/components/SplitContainer.test.ts
public_interface:
  - "export function colorForHash(hash: string): string"
  - "export function colorForLane(laneIndex: number): string"
  - "export function layoutGraph( commits: GraphCommit[], options: { dx?:
    number; dy?: number; padX?: number; padY?: number; /** 当某 commit
    被展开（GitGraph inline 详情）时，在该 commit 下方多腾出的高度。 */ expandedHash?: string;
    expandedExtra?: number; } = {} ): LayoutOutput"
  - export interface GraphCommit
  - export interface LayoutOutput
  - export interface RenderedDot
  - export interface RenderedLine
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

# gitGraphLayout.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
