# Iteration 118 - passive SCM polling and current LAN Remote smoke

Date: 2026-08-03  
Status: implementation complete; release deferred by the daily publication freeze

## Root cause

`SourceControl` refreshed every cached repository whenever its tab remounted,
and filesystem watcher bursts could schedule another status request after a
2-second debounce. The existing 5-minute heartbeat therefore did not bound all
passive `get_scm_status` calls.

## Delivered

- Added a shared per-repository successful-status timestamp to `scmCache`.
- Passive Source Control remounts and watcher events now skip fresh snapshots
  and poll at most once per five minutes; normalized Windows roots share the
  same gate.
- Explicit user refresh, checkout, commit, stage, pull/push/sync and branch
  actions retain immediate refresh semantics.
- Removed status timestamps and snapshots when a root leaves the active context
  or is confirmed non-Git, preventing stale memory and future probes.

## Verification

- `pnpm exec vitest run src/lib/stores/scmCache.test.ts src/lib/stores/paneGitStatus.test.ts`: 39 passed.
- `pnpm check`: 0 errors, 0 warnings.
- `pnpm exec node scripts/rdg-remote-e2e.mjs --skip-build`: LAN desktop/mobile
  browser matrix passed; both paths had `canvas=true`, `ws=true`, and input /
  resize probes passed with `browserErrors=[]`.
- `pnpm exec node scripts/mobile-keyboard-e2e.mjs`: Chromium mobile emulation
  passed keyboard convergence, recovery, touch selection, and zero browser
  errors. This is not physical-device evidence.

## Remaining gates

Public WebRTC, physical phone/PWA, WebView2 heap soak, dual-window/dual-host
and authenticated Git push evidence remain external. No version bump or
publication was made because `v0.1.54` consumed today's release allowance.
