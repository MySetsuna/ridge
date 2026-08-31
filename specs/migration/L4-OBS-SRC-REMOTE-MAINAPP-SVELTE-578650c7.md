---
id: L4-OBS-SRC-REMOTE-MAINAPP-SVELTE-578650c7
level: L4
parent: L3-OBS-SRC-REMOTE-2674e0ea
title: MainApp.svelte
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/remote/MainApp.svelte
test_targets:
  - src/remote/BottomTabBar.test.ts
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
  - src/remote/main.test.ts
  - src/remote/MainApp.test.ts
  - src/remote/pwaInstallScope.test.ts
  - src/remote/queryPolicy.test.ts
  - src/remote/runtimeMessagingScope.test.ts
verified_by:
  - TEST-OBS-SRC-REMOTE-BOTTOMTABBAR-TEST-TS-fad01c6a
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
  - TEST-OBS-SRC-REMOTE-MAIN-TEST-TS-d14db7b1
  - TEST-OBS-SRC-REMOTE-MAINAPP-TEST-TS-5cd570fc
  - TEST-OBS-SRC-REMOTE-PWAINSTALLSCOPE-TEST-TS-e8d7abb5
  - TEST-OBS-SRC-REMOTE-QUERYPOLICY-TEST-TS-a5d8de80
  - TEST-OBS-SRC-REMOTE-RUNTIMEMESSAGINGSCOPE-TEST-TS-e4055737
---

# MainApp.svelte

Remote mobile refresh settles the current VisualViewport before remeasuring and re-rendering the active terminal. Lazy scrollback fetch, validation, prepend, and cursor advancement form one latest-pane transaction: stale pages never mutate a kernel and failed prepends never advance the transport cursor.
