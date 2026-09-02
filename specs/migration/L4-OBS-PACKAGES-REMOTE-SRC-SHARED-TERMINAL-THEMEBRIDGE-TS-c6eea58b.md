---
id: L4-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-THEMEBRIDGE-TS-c6eea58b
level: L4
parent: L3-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-b1abe747
title: themeBridge.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/remote/src/shared/terminal/themeBridge.ts
test_targets:
  - packages/remote/src/shared/terminal/clipboardImage.test.ts
  - packages/remote/src/shared/terminal/dropPaste.test.ts
  - packages/remote/src/shared/terminal/fontDataService.test.ts
  - packages/remote/src/shared/terminal/fontStack.test.ts
  - packages/remote/src/shared/terminal/hostRemountPolicy.test.ts
  - packages/remote/src/shared/terminal/imeAnchor.test.ts
  - packages/remote/src/shared/terminal/imeDelta.test.ts
  - packages/remote/src/shared/terminal/initialPaneFit.test.ts
  - packages/remote/src/shared/terminal/linkAffordance.test.ts
  - packages/remote/src/shared/terminal/linkOpenHost.test.ts
  - packages/remote/src/shared/terminal/linkSpans.test.ts
  - packages/remote/src/shared/terminal/manager.attach.test.ts
  - packages/remote/src/shared/terminal/manager.test.ts
  - packages/remote/src/shared/terminal/mobileCopy.test.ts
  - packages/remote/src/shared/terminal/mobileTouchScroll.test.ts
  - packages/remote/src/shared/terminal/mouseForwardPolicy.test.ts
  - packages/remote/src/shared/terminal/paneDockResolve.test.ts
  - packages/remote/src/shared/terminal/paneGeometry.test.ts
  - packages/remote/src/shared/terminal/paneInputGate.test.ts
  - packages/remote/src/shared/terminal/paneOrigin.test.ts
  - packages/remote/src/shared/terminal/paneShell.test.ts
  - packages/remote/src/shared/terminal/ptyBridge.test.ts
  - packages/remote/src/shared/terminal/sentenceBuffer.test.ts
  - packages/remote/src/shared/terminal/shellInputSnapshot.test.ts
  - packages/remote/src/shared/terminal/terminalFeedPolicy.test.ts
  - packages/remote/src/shared/terminal/terminalFocus.test.ts
  - packages/remote/src/shared/terminal/terminalMemoryPolicy.test.ts
  - packages/remote/src/shared/terminal/themeBridge.test.ts
  - packages/remote/src/shared/terminal/tuiGate.test.ts
public_interface:
  - "export function pushTerminalThemeNow(): void"
  - "export function setupTerminalThemeBridge(): () => void"
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-CLIPBOARDIMAGE-TEST-TS-53411e31
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-DROPPASTE-TEST-TS-510e73ab
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-FONTDATASERVICE-TEST-TS-a11e69bb
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-FONTSTACK-TEST-TS-65f97da6
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-HOSTREMOUNTPOLICY-TEST-TS-117c2a90
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-IMEANCHOR-TEST-TS-105d9abf
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-IMEDELTA-TEST-TS-67f86c2c
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-INITIALPANEFIT-TEST-TS-fe5c49cf
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-LINKAFFORDANCE-TEST-TS-d1be43bc
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-LINKOPENHOST-TEST-TS-19b69752
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-LINKSPANS-TEST-TS-8d7f240f
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-MANAGER-ATTACH-TEST-TS-35aae100
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-MANAGER-TEST-TS-290fe11f
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-MOBILECOPY-TEST-TS-4906f450
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-MOBILETOUCHSCROLL-TEST-TS-ddb6edb0
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-MOUSEFORWARDPOLICY-TEST-TS-ddc8b0dc
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-PANEDOCKRESOLVE-TEST-TS-4fdcb558
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-PANEGEOMETRY-TEST-TS-ad29e3d7
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-PANEINPUTGATE-TEST-TS-06a1530b
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-PANEORIGIN-TEST-TS-6d99d562
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-PANESHELL-TEST-TS-e9209065
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-PTYBRIDGE-TEST-TS-3f30eb8e
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-SENTENCEBUFFER-TEST-TS-4bcd3337
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-SHELLINPUTSNAPSHOT-TEST-TS-9e6558fc
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-TERMINALFEEDPOLICY-TEST-TS-a929e750
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-TERMINALFOCUS-TEST-TS-ff98409c
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-TERMINALMEMORYPOLICY-TEST-TS-19d7b4ce
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-THEMEBRIDGE-TEST-TS-4fb837c0
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-TUIGATE-TEST-TS-146c1508
---

# themeBridge.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
