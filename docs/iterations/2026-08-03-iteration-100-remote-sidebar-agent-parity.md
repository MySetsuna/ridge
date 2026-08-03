# Iteration 100 — Remote sidebar Query and Agent Commune parity

Date: 2026-08-03  
Status: implementation complete; release intentionally deferred by the daily
publication cap

## Scope

This slice carried forward the approved mobile Remote/PWA and Agent Commune
requirements without creating a new version. It closes the deterministic code
gaps while keeping physical-device, public-network, and installed-PWA evidence
as explicit external gates.

## Delivered

- Remote Git/File sidebar reads use stable session/workspace/pane/path keys,
  Query single-flight and a 30-second stale window. Transport failures are not
  retried by the Query layer; user refresh uses the same key with `staleTime=0`.
  Successful writes invalidate only the scoped sidebar prefix.
- Non-Git detection remains a per-provider negative cache until the cwd/root
  changes, so status/branch/stash probes do not resume in a non-Git directory.
- Browser and standalone PWA layout now pins the app to the dynamic viewport and
  preserves safe-area ownership for drawer controls and the bottom action bar.
  Installation remains browser-native; no in-app install prompt was added.
- Agent roster DTOs carry host-authoritative pane CWD, with a live-pane fallback
  for older hosts. Remote cards expose CWD, status/history, and group controls.
  Group writes are optimistic but serialized, preserving rapid edits and
  rolling back rejected final mutations. Leader, color, and ordering operations
  share pure model helpers and deterministic tests.

## Verification

- Full Vitest: 144 files, 1486 passed, 1 skipped.
- `pnpm check`: 0 errors, 0 warnings.
- `cargo test -p ridge --lib commands::teammate`: 8 passed.
- `pnpm build:remote:mobile`: production mobile/PWA build succeeded; service
  worker generated.
- `pnpm build`: production desktop build succeeded (existing chunk-size
  warnings only).
- `pnpm e2e:rdg-lan`: desktop and mobile LAN paths passed (`canvas`, workspace
  tree, and WebSocket checks); generated artifacts were restored and are not
  part of the commit.
- Commits: `1045165` (Remote Query/PWA), `89cfeae` (Agent Commune parity), both
  pushed to `origin/main`; worktree clean.

## Remaining gates

Physical notch-device/PWA-installed interaction, public Cloud/WebRTC four-path
Remote evidence, WebView2 heap soak, branch-authoritative Query key population,
and full dual-window/dual-Host proof remain pending. They are not inferred from
local tests or the formal `v0.1.53` release.

