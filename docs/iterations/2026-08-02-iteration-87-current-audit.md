# Iteration 87 — Current completion audit

Date: 2026-08-02

This checkpoint follows the NLM iteration workflow. It records code and test
facts only; NotebookLM is strategy input, not release or runtime evidence.

## NLM inputs

- `nlm-iteration80-agent-commune-response.json`: recommends a bounded,
  identity-based session registry/view-model; cards, grouping, resume and pane
  border must derive from one projection. It explicitly warns against a broad
  kernel rewrite in this slice.
- `nlm-iteration79-response.json`: recommends Kernel lifecycle/single-instance
  correctness first, then domain authority and the shared MCP engine; it marks
  no-Tauri process-chain and contention runs as unverified.

## Current code facts

| Area | Evidence | State |
| --- | --- | --- |
| Remote Agent card CWD | `remote_host_impl.rs` emits pane `cwd`; `PaneInfo` carries it; `MainApp.svelte` passes `panes`; `SidebarTeamRoster.svelte:cwdFor/member-cwd` maps by `paneId` and truncates with `title`; `SidebarTeamRoster.test.ts` guards it | Implemented locally; physical mobile display still needs device evidence |
| Agent status rail / pane border | `SidebarTeamRoster` keeps the status left rail; desktop `SplitContainer` and Remote `TerminalCanvas` paint a border only from transient waiting/stopped intervention state; focus, claim, stdin or resize clears it | Implemented locally; normal running/idle panes have no border; physical mobile display still needs device evidence |
| Agent groups/history | Remote Members/Groups/History tabs, workspace-scoped group persistence, Agent-keyed history with `sessionId`/`cwd` | Implemented locally; real LAN/cloud mobile parity and structured resume remain external gates |
| Query-managed Remote data | `remoteQueries.ts` stable session/workspace/path keys, stale cache, single-flight and invalidation; sidebar file/Git/search/diff use it | Implemented locally |
| Git commit/push/Graph | capability-gated mutation surface with confirmation/cancel/progress; shared graph renderer; Remote transport preserves refs/branch/HEAD and selected commit author/date/parents; Agent group deletion is confirmed and persisted | Implemented locally; authenticated Remote push and public artifact republish remain external gates |
| PWA | `pwaInstallScope.test.ts` forbids app install button and `beforeinstallprompt` ownership; manifest/SW/standalone/scope remain; drawer safe-area contract is tested | Implemented per latest user correction; browser-native installation is intentionally out of business E2E |
| Mobile `runtime.lastError` | Source audit found no Chrome Extension Messaging API; service worker uses standard `clients.matchAll`/`Client.postMessage` | No business-code fix is authorized; affected-phone source URL and clean-profile/extension A/B remain required |
| Kernel singleton | `KernelInstanceGuard` uses process-lifetime OS lock; `registry.rs` child-process probe proves a second process cannot acquire the lock (`c692781`) | Deterministic guard implemented; real shell death/deep-root no-Tauri chain remains unverified |
| Release / Remote | Release `v0.1.35` has 12 matching assets; Remote workflow `30736163197` activated `0.1.35+g2bce084`; cloud health is `ok` but reports service `0.0.7` | Published evidence exists; no cloud source/version change was fabricated |

## User-visible correction captured

The active requirement now states that Remote must not render an “Add to Home
Screen” or Install App button and must not consume `beforeinstallprompt`.
Installation remains the browser's native responsibility. Remote owns only
standalone/PWA layout: safe-area, notch, rotation, keyboard and theme.

## Remaining gates (not falsely closed)

1. Affected-phone `runtime.lastError` source URL plus clean-profile and
   one-extension-at-a-time A/B.
2. Physical iOS/Android notch/PWA keyboard and touch run.
3. Public Remote soak, dual-window/dual-host workspace singleton, and
   authenticated Remote Git push.
4. WebView2 long-run heap/resource snapshot and real no-Tauri Kernel → rdg →
   ridge-mcp process chain.
5. Remote artifact republish for the latest GitGraph and transient pane-attention
   code; desktop Release stays at `v0.1.35` without a version bump.

No Console suppression, third-party extension mutation, fake PWA install
state, or physical-device claim is made.
