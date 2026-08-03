# Iteration 130 — PWA connection notice safe-area isolation

Date: 2026-08-03

## Outcome

Remote reconnect and connection-failure notices no longer put their content
box at the viewport's cutout edge. A dedicated safe-area flex item reserves
the top inset before the notice, with `env()`/legacy `constant()` and a 44px
standalone fallback. LAN/browser mode keeps the normal compact banner. Auth and
cloud-gate fallback screens also consume the iOS `navigator.standalone` marker.

## Evidence

- `pnpm exec vitest run src/remote/pwaInstallScope.test.ts --reporter=dot`:
  5/5 passed.
- `pnpm check`: 0 errors / 0 warnings.
- `pnpm build:remote:mobile` and
  `node scripts/verify-remote-pwa-build.mjs`: exit `0`; all PWA/safe-area
  checks true.
- Commit `e22c450` pushed to `origin/main`.

## Boundary

The fix is source/build verified; physical iPhone notch and installed WebView2
interaction still require device evidence. No version bump or publication was
made because `v0.1.54` consumed today's release allowance.

