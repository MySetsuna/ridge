# Iteration 115 - PWA connection notice safe-area compatibility

Date: 2026-08-03  
Status: implementation complete; release deferred by the daily publication freeze

## Root cause

The mobile reconnect/failure notice relied on one `env()` declaration and fixed
horizontal padding. Standalone WebKit variants that still expose the cutout via
`constant()` could leave the action row in the display cutout; landscape cutouts
could also consume the fixed 12px side padding.

## Delivered

- Keep the connection banner's text and action inside top, left, and right safe
  areas; allow the notice to wrap on narrow PWA viewports.
- Add `constant()` fallback declarations before the modern `env()` declarations
  for both the in-app connection banner and LAN/cloud auth fallback screens.
- Preserve browser-native PWA installation and add source-level regression guards
  for the legacy safe-area path.

## Verification

- `pnpm exec vitest run src/remote/pwaInstallScope.test.ts src/remote/BottomTabBar.test.ts src/remote/lib/RemoteSidebar.test.ts`: 7 passed.
- `pnpm check`: 0 errors, 0 warnings.
- `pnpm build:remote:mobile`: succeeded; PWA evidence reports viewport-fit,
  standalone manifest, generated service worker, and safe-area CSS.

## Remaining gates

Physical notch-device/PWA evidence remains external; no version bump or
publication was made because `v0.1.54` consumed today's release allowance.
