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
| Agent status rail / pane border | `SidebarTeamRoster` keeps the status left rail; desktop `SplitContainer` and Remote `TerminalCanvas` paint a border only from transient waiting/stopped intervention state; focus, claim, stdin or resize clears it; `SplitContainer.test.ts` and `TerminalCanvas.test.ts` guard the contract | Implemented locally; normal running/idle panes have no border; physical mobile display still needs device evidence |
| Agent groups/history | Remote Members/Groups/History tabs, workspace-scoped group persistence, Agent-keyed history with `sessionId`/`cwd` | Implemented locally; real LAN/cloud mobile parity and structured resume remain external gates |
| Query-managed Remote data | `remoteQueries.ts` stable session/workspace/path keys, stale cache, single-flight and invalidation; sidebar file/Git/search/diff use it | Implemented locally |
| Git commit/push/Graph | capability-gated mutation surface with confirmation/cancel/progress; shared graph renderer; Remote transport preserves refs/branch/HEAD and selected commit author/date/parents; real temp-repo commit/push and non-fast-forward rejection now exercise the shared Rust handlers | Implemented locally; authenticated Remote push and public artifact republish remain external gates |
| Remote Host connect / attach | `HostConnectDialog` closes before discovery; persistent Hosts panel renders connection/workspace progress; `hostSessionDrag` and button attach share `attachHostSession`; remote attach schedules first DOM-measured pane-size synchronization; `hostConnectFlow.test.ts` guards the path | Implemented locally; public/physical Host latency and drag/resize evidence remain external |
| PWA | `pwaInstallScope.test.ts` forbids app install button and `beforeinstallprompt` ownership; manifest/SW/standalone/scope remain; drawer safe-area contract is tested | Implemented per latest user correction; browser-native installation is intentionally out of business E2E |
| Mobile `runtime.lastError` | Source audit found no Chrome Extension Messaging API; service worker uses standard `clients.matchAll`/`Client.postMessage`; controlled LAN browser matrix now explicitly disables extensions and component extensions | No business-code fix is authorized; controlled clean-profile run is green, but affected-phone source URL and one-by-one extension A/B remain required |
| Kernel singleton | `KernelInstanceGuard` uses process-lifetime OS lock; `registry.rs` child-process probe proves a second process cannot acquire the lock (`c692781`) | Deterministic guard implemented; real shell death/deep-root no-Tauri chain remains unverified |
| Release / Remote | Release `v0.1.36` has 12 matching assets; Remote workflow `30739703846` activated `0.1.36+gf7ba0f5`; cloud health is `ok` but reports service `0.0.7` | Published evidence exists; no cloud source/version change was fabricated |

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
5. Authenticated Remote Git push, public soak and physical-device evidence remain
   external; latest Remote artifact is activated and desktop Release `v0.1.36`
   is formal with matching assets.

No Console suppression, third-party extension mutation, fake PWA install
state, or physical-device claim is made.

## Local gate result

- `pnpm exec vitest run --reporter=dot`: 137 files, 1,439 passed, 1 skipped.
- `pnpm check`: 0 errors, 0 warnings.
- `cargo test -p ridge-kernel registry::tests --lib`: 4 passed.
- `cargo check --manifest-path src-tauri/Cargo.toml`: finished successfully;
  existing Rust warnings only.

## Iteration 87 continuation — memory, focus acknowledgement and LAN control path

- `e41e733`, `cea02b9`, `9aa23dc`, and `ab01e66` harden terminal reclamation:
  Scrollback drops its backing ring on clear and lazily reallocates on output;
  link-span rows and local right-click clear release immediately; worker decode
  faults and invalid ranges return request-scoped errors instead of leaving a
  pending decode until the 10-second timeout. Deterministic evidence: ridge-term
  Scrollback 48 tests, then 8 and 9 scrollback-worker tests, all passing.
  WebView2/mobile heap soak and allocator RSS return remain external gates.
- `4c7fb4f` completes the transient Agent attention contract on mobile: the
  hidden terminal input now acknowledges the pane on real focus, in addition to
  Agent selection, stdin, claim and Resize. Normal working/idle panes remain
  border-free; `pnpm check` stays 0 errors/0 warnings.
- `b20ea58` and `144a467` fence LAN probe ownership with per-process status files,
  spawned PID/port checks, teardown cleanup and contract tests. The latest
  `pnpm e2e:rdg-lan` passed both desktop and mobile dashboard paths with
  `browserErrors=[]`, `write_to_pty` and `resize_pane` frames observed on both
  clients. Evidence is `.iteration/artifacts/rdg-remote-e2e/last-result.json`.
  `pnpm e2e:rdg-mobile-keyboard` also passed Chromium emulation with selection,
  bounded keyboard shift and recovery; this is not a physical-device claim.
- Final local gate rerun after these commits: full Vitest `137 files / 1,439
  passed / 1 skipped`, `pnpm check` 0/0, `cargo test -p ridge-term scrollback
  --lib` 48 passed, `cargo test -p ridge-kernel --lib` 17 passed,
  `cargo test -p ridge-mcp-bridge --lib` 8 passed, and `cargo check
  --manifest-path src-tauri/Cargo.toml` exit 0 (39 pre-existing warnings).
- Kernel deep-root evidence is in
  `.iteration/agents/result-kernel-deep-root.json`: singleton, no-Tauri host,
  rdg attach/stop, kernel-backed FS/Agent/Git/MCP and standalone MCP bridge
  probes passed. The desktop workspace/teammate domain is still partly
  AppState-backed, so full kernel-domain migration is not closed.

The public WebRTC four-path gate remains blocked by the absence of an
authenticated `rdg login`/device credential on this machine. Cloud health HTTP
200 and a LAN protocol smoke do not substitute for public desktop/mobile session
evidence.

Runtime warning attribution update: `scripts/rdg-remote-e2e.mjs` now launches
Chromium with `--disable-extensions` and
`--disable-component-extensions-with-background-pages`, and records the
isolation mode in the evidence JSON; temporary TOTP/auth material is redacted
from logs and artifacts. The latest desktop/mobile matrix passed
with `browserErrors=[]`, real `write_to_pty`/`resize_pane` frames and
`browserIsolation.extensionsDisabled=true`. This excludes the controlled
project path in a no-extension profile; it does not identify the affected
physical phone's injector, so no third-party or business-code attribution is
claimed.

The local source guard `src/remote/runtimeMessagingScope.test.ts` now asserts
that the Remote entrypoint and service worker contain no Chrome Extension
Messaging APIs (`chrome.runtime`, `chrome.tabs`, `sendResponse`, or
`runtime.lastError`) and continue to use one-way service-worker
`Client.postMessage`. This is a deterministic repository guard; it does not
replace attribution on the affected phone.

Git mutation evidence update: `packages/ridge-core/src/commands/git.rs` now
guards a real temporary-repository test through the same `git_commit_sync` and
`git_push_sync` handlers used by the UI. It verifies local bare-remote push
success and a later non-fast-forward push failure; no user repository or
network credential is used. Authenticated Remote Git push remains external.

Release gate: the first `v0.1.36` attempt failed before the matrix because
`Cargo.lock` had an invalid `tracing-core` resolution; the tag was removed,
the lockfile was corrected, and the fixed annotated tag was rebuilt. Workflow
`30738592676` passed test gate plus Windows/Linux/macOS arm64/x64 builds; formal
Release `v0.1.36` is published with 12 matching assets.

Remote artifact publish gate: workflow `30739703846` succeeded from `f7ba0f5`
and activated `0.1.36+gf7ba0f5` with both desktop/mobile indexes. Cloud
`/api/v1/health` returned HTTP 200 (`version=0.0.7`); no cloud source/version
change was fabricated.
