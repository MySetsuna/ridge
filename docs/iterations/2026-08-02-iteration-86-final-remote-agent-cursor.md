# Iteration 86 — Remote Agent/Cursor Final Checkpoint

Date: 2026-08-02

## Delivered

- `e344e57` keeps the desktop IME helper as the sole visible native caret while
  its pane is focused; renderer focus is restored on blur without stealing the
  keyboard.
- `478c047` and `a4c146e` project teammate state and identity through the
  remote pane contract. Agent cards now resolve and render their live `cwd`,
  and pane containers show state-aware outer borders (`busy`, `starting`,
  `idle`). Workspace-tree rows use the same state signal.
- `f129d81` changes the Agent roster liveness refresh from a 5-second loop to a
  five-minute interval. TanStack Query remains the cache/single-flight layer;
  opening the drawer does not issue an unconditional fresh RPC.
- `d818500` adds a source contract test for the five-minute interval.

## Key call path

`build_remote_pane_list` → `PaneInfo { cwd, agentId, agentState }` →
`MainApp.svelte` → `TerminalCanvas.svelte` border/caret, and
`RemoteSidebar.svelte` → `SidebarTeamRoster.svelte` card CWD/status rail.

HITL approval status remains rendered on the Agent card's yellow left rail and
approval controls. Pane borders represent the pane lifecycle state received
from the host; no unverified pane-level HITL mapping is claimed.

## Verification

- `pnpm check`: 0 errors, 0 warnings.
- Full Vitest: 131 files passed; 1,418 passed, 1 skipped.
- Targeted Agent/Query tests: 2 files, 6 passed.
- Remote publish workflow `30736163197` succeeded from `2bce084`.
- Activated Remote artifact: `0.1.35+g2bce084`; desktop/mobile indexes passed.
- GitHub Release `v0.1.35` is formal (`draft=false`) with all 12 expected
  Windows/Linux/macOS installer and CLI assets.

## Explicit external gates

Physical notch-phone/PWA install interaction, public-network soak, WebView2
heap soak, dual-window/dual-host E2E, authenticated real Git push, and clean
browser-profile extension attribution still require their respective real
environments. Tauri is currently a compatibility shell over local host/PTY
services, not yet a pure UI shell.
