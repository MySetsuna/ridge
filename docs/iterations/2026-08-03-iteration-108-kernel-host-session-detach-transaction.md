# Iteration 108 - Kernel host session detach transaction

Date: 2026-08-03  
Status: implementation complete; release deferred by the daily publication freeze

## Root cause

`detach_foreign` removed the local foreign-pane mapping first and then used the
legacy best-effort full-record mirror for `session.attached=false`. A failed or
unavailable kernel write could therefore leave the remote session attached
while the local PTY, sink, and subscription were already being destroyed.

## Delivered

- Serialized attach and detach through the existing session transaction lock.
- Detached the kernel session first through the typed atomic endpoint; local
  foreign metadata, PTY input sink, live output buffer, backpressure state, and
  outbound subscription are cleaned only after that transition succeeds.
- Kept the complete local attachment intact when the kernel transition fails,
  allowing a deterministic retry instead of split-brain cleanup.
- Added injectable ordering/failure tests so the production sequencing is
  covered without requiring a live kernel process in unit tests.
- Removed the user-visible detach path from the legacy full-record setter;
  that setter remains only for best-effort reconnect/disconnect projection.

## Verification

- `cargo test -p ridge --lib hosts::tests --quiet`: 18 passed.
- `cargo test -p ridge-kernel --lib --quiet`: 24 passed.
- `pnpm check`: 0 errors / 0 warnings.
- `git diff --check`: passed.
- No version bump or publication was made because `v0.1.54` consumed today's
  release allowance.

## Remaining gates

Live PTY ownership, transport lifecycle, cross-process workspace/window claims,
and physical/public Remote evidence still need their external gates; this
follow-up does not claim those gates complete.
