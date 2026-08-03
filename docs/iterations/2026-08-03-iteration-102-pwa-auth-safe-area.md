# Iteration 102 — PWA reconnect fallback safe area

Date: 2026-08-03  
Status: implementation complete; release deferred by the daily publication freeze

## Root cause

The previous iteration protected `MainApp`'s reconnect and failure banners, but
the failure path can unload `MainApp` and return to either `AuthScreen` (LAN) or
`CloudAuthScreen` (public Cloud). Those fixed full-viewport gates still had a
plain `24px` padding box, so connection details and retry/login controls could
land beneath an iPhone/Android display cutout in standalone PWA mode.

## Delivered

- Added top/bottom safe-area padding plus horizontal cutout padding to both auth
  gates.
- Added bounded vertical scrolling and `overscroll-behavior: contain`, so long
  reconnect diagnostics remain reachable on short viewports.
- Added deterministic PWA scope coverage for both fallback screens; the existing
  `MainApp` banner, bottom bar, and drawer contracts remain covered.

## Verification

- Targeted PWA scope tests: 4 passed.
- Full Vitest: 144 files, 1490 passed, 1 skipped.
- `pnpm check`: 0 errors, 0 warnings.
- `pnpm build:remote:mobile`: production PWA build succeeded; service worker
  generated with 36 precache entries.
- `pnpm e2e:rdg-lan`: desktop and mobile flows passed (`canvas`, tree, WS).

## Release and archive

Changes are local to the Remote PWA UI and are ready for the next allowed
publication window. No version bump or artifact publication was made because
`v0.1.54` already consumed today's release allowance. This note is archived by
the project-state index and is not a pending requirement.
