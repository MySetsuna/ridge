---
id: L4-OBS-SRC-REMOTE-LIB-THEME-TS-8d9c60f3
level: L4
parent: L3-OBS-SRC-REMOTE-LIB-f2853d2d
title: theme.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/remote/lib/theme.ts
test_targets:
  - packages/remote/src/shared/terminal/themeBridge.test.ts
  - src/lib/components/customTheme.test.ts
  - src/lib/monaco/ridgeTheme.test.ts
  - src/lib/stores/themes.slug.test.ts
  - src/lib/stores/themes.test.ts
  - src/remote/lib/clipboard.test.ts
  - src/remote/lib/cloudRemote.test.ts
  - src/remote/lib/generationGuard.test.ts
  - src/remote/lib/keyboardOffset.test.ts
  - src/remote/lib/listenerCleanup.test.ts
  - src/remote/lib/paneFeedScheduler.test.ts
  - src/remote/lib/paneLifecycle.test.ts
  - src/remote/lib/paneSwitchBuffer.test.ts
  - src/remote/lib/remoteGitActions.test.ts
  - src/remote/lib/RemoteGitPanel.test.ts
  - src/remote/lib/remoteQueries.test.ts
  - src/remote/lib/RemoteSidebar.test.ts
  - src/remote/lib/scrollbackWorker.test.ts
  - src/remote/lib/sidebarProvider.test.ts
  - src/remote/lib/SidebarTeamRoster.test.ts
  - src/remote/lib/teamRosterScope.test.ts
  - src/remote/lib/TerminalCanvas.test.ts
  - src/remote/lib/theme.test.ts
  - src/remote/lib/treeState.test.ts
  - src/remote/lib/WorkspaceTree.test.ts
  - tests/e2e-shell/theme-injection.spec.ts
  - tests/e2e-shell/theme-rotation.spec.ts
public_interface:
  - "export function applyThemeVars(colors: Record<string, string>): void"
  - "export function buildKernelTheme(colors: Record<string, string>):
    Record<string, string>"
  - "export function themeChromeColor(colors: Record<string, string>): string |
    null"
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-THEMEBRIDGE-TEST-TS-4fb837c0
  - TEST-OBS-SRC-LIB-COMPONENTS-CUSTOMTHEME-TEST-TS-0b73284d
  - TEST-OBS-SRC-LIB-MONACO-RIDGETHEME-TEST-TS-3b597c0b
  - TEST-OBS-SRC-LIB-STORES-THEMES-SLUG-TEST-TS-146943ed
  - TEST-OBS-SRC-LIB-STORES-THEMES-TEST-TS-8b9fb08d
  - TEST-OBS-SRC-REMOTE-LIB-CLIPBOARD-TEST-TS-acf18327
  - TEST-OBS-SRC-REMOTE-LIB-CLOUDREMOTE-TEST-TS-8f8cb6d4
  - TEST-OBS-SRC-REMOTE-LIB-GENERATIONGUARD-TEST-TS-954c7584
  - TEST-OBS-SRC-REMOTE-LIB-KEYBOARDOFFSET-TEST-TS-aa391cba
  - TEST-OBS-SRC-REMOTE-LIB-LISTENERCLEANUP-TEST-TS-b750577c
  - TEST-OBS-SRC-REMOTE-LIB-PANEFEEDSCHEDULER-TEST-TS-6040ce95
  - TEST-OBS-SRC-REMOTE-LIB-PANELIFECYCLE-TEST-TS-b323d4f0
  - TEST-OBS-SRC-REMOTE-LIB-PANESWITCHBUFFER-TEST-TS-8eee2043
  - TEST-OBS-SRC-REMOTE-LIB-REMOTEGITACTIONS-TEST-TS-e28d50f2
  - TEST-OBS-SRC-REMOTE-LIB-REMOTEGITPANEL-TEST-TS-40e79e76
  - TEST-OBS-SRC-REMOTE-LIB-REMOTEQUERIES-TEST-TS-42897c52
  - TEST-OBS-SRC-REMOTE-LIB-REMOTESIDEBAR-TEST-TS-743bbf5b
  - TEST-OBS-SRC-REMOTE-LIB-SCROLLBACKWORKER-TEST-TS-a458774b
  - TEST-OBS-SRC-REMOTE-LIB-SIDEBARPROVIDER-TEST-TS-3032da1a
  - TEST-OBS-SRC-REMOTE-LIB-SIDEBARTEAMROSTER-TEST-TS-65740552
  - TEST-OBS-SRC-REMOTE-LIB-TEAMROSTERSCOPE-TEST-TS-a407e9aa
  - TEST-OBS-SRC-REMOTE-LIB-TERMINALCANVAS-TEST-TS-e83a9e51
  - TEST-OBS-SRC-REMOTE-LIB-THEME-TEST-TS-74f6feb0
  - TEST-OBS-SRC-REMOTE-LIB-TREESTATE-TEST-TS-bcbf35f8
  - TEST-OBS-SRC-REMOTE-LIB-WORKSPACETREE-TEST-TS-9753d5fd
  - TEST-OBS-TESTS-E2E-SHELL-THEME-INJECTION-SPEC-TS-ef7845cc
  - TEST-OBS-TESTS-E2E-SHELL-THEME-ROTATION-SPEC-TS-6e476756
---

# theme.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
