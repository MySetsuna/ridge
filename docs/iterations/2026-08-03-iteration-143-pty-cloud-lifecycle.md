# Iteration 143 — PTY and Cloud pane lifecycle guards

Date: 2026-08-03

## Scope

Close two concrete lifecycle races found during the iteration-142 audit:

1. Concurrent `ensurePtyBridge` calls could create duplicate Tauri listeners and
   delta Channels for one Pane. A real close during asynchronous registration
   could leave a listener alive after the Pane was destroyed.
2. Cloud raw-pane subscription could race listener setup and teardown. A host
   could receive `unsubscribe_pane_raw` before an in-flight subscribe and retain
   the stream.

## Changes

- `packages/remote/src/shared/terminal/ptyBridge.ts`
  - Add one in-flight attach promise per stable Pane key.
  - Add teardown cancellation checks after each asynchronous listener/channel
    boundary; acquired listeners are released before an attach is published.
- `packages/remote/src/shared/cloud/cloudHostPaneSource.ts`
  - Register the event listener before starting the host subscription.
  - Serialize unsubscribe after subscribe completion, including rejected and
    synchronous-failure paths.
  - Contain output callback and source errors behind the injected diagnostic log.
- Tests cover concurrent attach single-flight, close-during-listen cancellation,
  listener-before-subscribe ordering, subscribe/unsubscribe ordering, and source
  failure containment.

## Verification

- `pnpm exec vitest run packages/remote/src/shared/cloud/cloudHostBridge.test.ts packages/remote/src/shared/cloud/cloudHostPaneSource.test.ts packages/remote/src/shared/terminal/ptyBridge.test.ts`
  - 3 files, 80 tests passed (the PowerShell wrapper surfaced expected stderr
    diagnostics from an existing no-source test; Vitest itself exited green).
- `pnpm check`: 0 errors / 0 warnings.
- Full Vitest: 147 files / 1529 passed / 1 skipped.
- `cargo test -p ridge-cli --bin rdg --quiet`: 129 passed.
- `cargo test -p ridge-kernel --lib --quiet`: 31 passed.
- `git diff --check`: passed.

## Release and external gates

No version bump, release, Remote cloud publication, or public deployment was
made. `v0.1.54` consumed today's release allowance. Physical phone/public
Remote, WebView2 memory soak, dual-window/dual-host, and full Kernel-domain
authority evidence remain open and are not inferred from unit tests.
