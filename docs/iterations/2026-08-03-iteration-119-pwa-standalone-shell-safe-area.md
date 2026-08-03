# Iteration 119 - standalone PWA shell safe-area contract

Date: 2026-08-03  
Status: implementation complete; release deferred by the daily publication freeze

## Root cause

Some iOS standalone windows expose the PWA state through
`navigator.standalone` while `(display-mode: standalone)` remains false. The
connection banner therefore painted at the viewport origin and its reconnect,
failure, and retry controls could sit under the cutout.

## Delivered

- Detect standalone mode before the first Svelte paint, including the iOS
  `navigator.standalone` path, and mark the document with
  `data-ridge-pwa="standalone"`.
- Apply the conservative 44px portrait top belt (or the larger real
  `env()`/`constant()` inset) to reconnect/failure banners and the mobile
  header.
- Apply matching standalone top/bottom fallbacks to the Remote drawer and
  bottom action bar, keeping controls tappable around cutouts and home
  indicators.

## Verification

- `pnpm exec vitest run src/remote/pwaInstallScope.test.ts --reporter=dot`: 5
  passed.
- `pnpm check`: 0 errors, 0 warnings.
- `pnpm build:remote:mobile`: succeeded.
- `pnpm exec node scripts/verify-remote-pwa-build.mjs`: safe-area CSS,
  standalone manifest, service worker, and no in-app install hook all verified.

Physical iPhone/notch-device evidence remains external. No version bump or
publication was made because `v0.1.54` consumed today's release allowance.
