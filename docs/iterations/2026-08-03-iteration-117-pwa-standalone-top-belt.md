# Iteration 117 - PWA standalone top-belt fallback

Date: 2026-08-03  
Status: implementation complete; release deferred by the daily publication freeze

## Root cause

Some standalone Android/WebView shells expose a display cutout while reporting
`env(safe-area-inset-top)` as zero. The reconnect and reconnect-failure banner
then remained flush with the physical status/cutout belt even though the normal
`env()` and legacy `constant()` declarations were present.

## Delivered

- Added a portrait standalone-only top-belt fallback (44px) for the live
  reconnect/failure banner.
- Kept actual `env()`/`constant()` insets authoritative through `max()`, so
  larger physical insets remain respected and normal browser mode is unchanged.
- Applied the same fallback to LAN and Cloud authentication/reconnect screens,
  which can render after MainApp is unloaded.
- Added source guards and verified the generated production CSS contains the
  standalone media and both inset branches.

## Verification

- `pnpm exec vitest run src/remote/pwaInstallScope.test.ts src/remote/BottomTabBar.test.ts src/remote/lib/RemoteSidebar.test.ts`: 7 passed.
- `pnpm check`: 0 errors, 0 warnings.
- `pnpm build:remote:mobile`: succeeded; generated CSS contains the standalone
  portrait fallback and PWA evidence reports safe-area CSS, standalone manifest,
  scope, service worker, and no in-app install hook.

## Remaining gates

Physical notch-device/PWA evidence remains external. No version bump or
publication was made because `v0.1.54` consumed today's release allowance.
