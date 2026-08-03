# Iteration 163 — Kernel Git authority for all desktop operations

## Scope

Iteration 162 left two classes of direct desktop Git paths: mutations beyond
stage/unstage/commit/checkout/push, and graph/history reads. This iteration
closes that authority gap without changing the Tauri command signatures used by
the UI or remote adapters.

## Change

- Extended the authenticated tagged `/v1/domain/git/mutate` contract to cover
  discard, clean, branch/tag operations, stash writes, fetch/pull/sync,
  cherry-pick/revert, tag creation, and reset.
- Added authenticated tagged `/v1/domain/git/read` for repository info,
  paginated commits, file versions, commit files, commit comparisons, diff
  files, blame, and file history.
- Added source-checked typed client adapters and explicit non-Git negative
  results. The kernel performs repository detection before starting a Git
  child; no arbitrary argv or local Tauri fallback is accepted.
- Migrated the desktop Tauri wrappers for every listed write and graph/history
  read to the kernel adapters while preserving existing command names,
  parameters, and result shapes.
- Added deterministic decoder and non-Git domain guard tests.

## Verification

- `cargo test -p ridge-kernel --lib`: full matrix recorded in the archive.
- `cargo check -p ridge --lib`: passed; existing warnings only.
- `cargo test -p ridge-core --lib`, Vitest, and `pnpm check`: full matrix
  recorded in the archive.
- `git diff --check` and the requirements/iteration gates pass.
- `pnpm build:remote:mobile`: passed; generated mobile PWA bundle and service
  worker.
- `pnpm verify:pwa`: passed (`standalone`, root scope, icons, service worker,
  safe-area CSS, and no in-app install hook).
- `pnpm e2e:rdg-lan`: passed for LAN desktop and mobile Remote paths with no
  browser errors.
- `pnpm e2e:rdg-mobile-keyboard`: passed for keyboard viewport convergence,
  touch selection, copy visibility, and clean Console under a fresh Chromium
  mobile context. This is emulation evidence, not physical-device proof.

## Remaining external gates

Physical/public Remote, WebView2 heap soak, dual-window workspace singleton,
remote Host attach/resize, and real mobile clean-profile extension attribution
still require their respective runtime environments. They are not claimed by
the source-level kernel migration or browser emulation.

No version bump or release is made in this iteration.
