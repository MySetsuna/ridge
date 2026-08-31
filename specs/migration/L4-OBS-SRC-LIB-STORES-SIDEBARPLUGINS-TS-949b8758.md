---
id: L4-OBS-SRC-LIB-STORES-SIDEBARPLUGINS-TS-949b8758
level: L4
parent: L3-OBS-SRC-LIB-STORES-c2ef71cf
title: sidebarPlugins.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/stores/sidebarPlugins.ts
test_targets:
  - src/lib/stores/clipboardResolve.test.ts
  - src/lib/stores/contextMenu.test.ts
  - src/lib/stores/dockRegionPicker.test.ts
  - src/lib/stores/editorWindow.test.ts
  - src/lib/stores/explorerLayout.test.ts
  - src/lib/stores/fileEditor.test.ts
  - src/lib/stores/fileExplorer.test.ts
  - src/lib/stores/fileWatcherSync.test.ts
  - src/lib/stores/fsEvents.test.ts
  - src/lib/stores/gitGuardStats.test.ts
  - src/lib/stores/hostReconnect.test.ts
  - src/lib/stores/hostReconnectProduct.test.ts
  - src/lib/stores/hosts.connect.test.ts
  - src/lib/stores/hosts.refresh.test.ts
  - src/lib/stores/hostsMutation.test.ts
  - src/lib/stores/hostsOutbound.test.ts
  - src/lib/stores/hostsOutboundProduct.test.ts
  - src/lib/stores/hostsPublic.test.ts
  - src/lib/stores/imagePreviewVersion.test.ts
  - src/lib/stores/paneGitStatus.test.ts
  - src/lib/stores/paneTree.coverage.test.ts
  - src/lib/stores/paneTree.test.ts
  - src/lib/stores/processGuardPolicy.test.ts
  - src/lib/stores/project.test.ts
  - src/lib/stores/remoteStatus.test.ts
  - src/lib/stores/scmCache.test.ts
  - src/lib/stores/searchState.test.ts
  - src/lib/stores/settings.test.ts
  - src/lib/stores/terminalHistory.test.ts
  - src/lib/stores/termSettings.test.ts
  - src/lib/stores/themes.slug.test.ts
  - src/lib/stores/themes.test.ts
public_interface:
  - "export function registerSidebarPlugin(plugin: SidebarPlugin): void"
  - "export function unregisterSidebarPlugin(id: string): void"
  - export interface SidebarPlugin
  - export interface SidebarPluginProps
  - export type SidebarPluginScope
verified_by:
  - TEST-OBS-SRC-LIB-STORES-CLIPBOARDRESOLVE-TEST-TS-d8cab7c6
  - TEST-OBS-SRC-LIB-STORES-CONTEXTMENU-TEST-TS-c5d624d2
  - TEST-OBS-SRC-LIB-STORES-DOCKREGIONPICKER-TEST-TS-b7ce6df6
  - TEST-OBS-SRC-LIB-STORES-EDITORWINDOW-TEST-TS-965f4022
  - TEST-OBS-SRC-LIB-STORES-EXPLORERLAYOUT-TEST-TS-489203e2
  - TEST-OBS-SRC-LIB-STORES-FILEEDITOR-TEST-TS-396c0017
  - TEST-OBS-SRC-LIB-STORES-FILEEXPLORER-TEST-TS-8df44237
  - TEST-OBS-SRC-LIB-STORES-FILEWATCHERSYNC-TEST-TS-9e135a5d
  - TEST-OBS-SRC-LIB-STORES-FSEVENTS-TEST-TS-f628ded6
  - TEST-OBS-SRC-LIB-STORES-GITGUARDSTATS-TEST-TS-680ef77e
  - TEST-OBS-SRC-LIB-STORES-HOSTRECONNECT-TEST-TS-e82e61be
  - TEST-OBS-SRC-LIB-STORES-HOSTRECONNECTPRODUCT-TEST-TS-9598a61c
  - TEST-OBS-SRC-LIB-STORES-HOSTS-CONNECT-TEST-TS-3c065b9c
  - TEST-OBS-SRC-LIB-STORES-HOSTS-REFRESH-TEST-TS-57fb3707
  - TEST-OBS-SRC-LIB-STORES-HOSTSMUTATION-TEST-TS-ef74d67d
  - TEST-OBS-SRC-LIB-STORES-HOSTSOUTBOUND-TEST-TS-afc6c610
  - TEST-OBS-SRC-LIB-STORES-HOSTSOUTBOUNDPRODUCT-TEST-TS-c9cf71df
  - TEST-OBS-SRC-LIB-STORES-HOSTSPUBLIC-TEST-TS-b49d4adf
  - TEST-OBS-SRC-LIB-STORES-IMAGEPREVIEWVERSION-TEST-TS-c18c0477
  - TEST-OBS-SRC-LIB-STORES-PANEGITSTATUS-TEST-TS-73bc40e2
  - TEST-OBS-SRC-LIB-STORES-PANETREE-COVERAGE-TEST-TS-747ca055
  - TEST-OBS-SRC-LIB-STORES-PANETREE-TEST-TS-8fb828ce
  - TEST-OBS-SRC-LIB-STORES-PROCESSGUARDPOLICY-TEST-TS-a94798ee
  - TEST-OBS-SRC-LIB-STORES-PROJECT-TEST-TS-b97586fc
  - TEST-OBS-SRC-LIB-STORES-REMOTESTATUS-TEST-TS-a66e6e00
  - TEST-OBS-SRC-LIB-STORES-SCMCACHE-TEST-TS-69c519de
  - TEST-OBS-SRC-LIB-STORES-SEARCHSTATE-TEST-TS-7c629e13
  - TEST-OBS-SRC-LIB-STORES-SETTINGS-TEST-TS-ebb9f869
  - TEST-OBS-SRC-LIB-STORES-TERMINALHISTORY-TEST-TS-49edc8b5
  - TEST-OBS-SRC-LIB-STORES-TERMSETTINGS-TEST-TS-222ac7cd
  - TEST-OBS-SRC-LIB-STORES-THEMES-SLUG-TEST-TS-146943ed
  - TEST-OBS-SRC-LIB-STORES-THEMES-TEST-TS-8b9fb08d
---

# sidebarPlugins.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
