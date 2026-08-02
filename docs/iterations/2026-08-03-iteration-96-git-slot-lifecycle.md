# Iteration 96 — Git request-slot lifecycle hardening

Date: 2026-08-03  
Status: code closed; release gate pending

## Scope

Close the remaining deterministic Git lifecycle gap identified during the
kernel/Remote stability audit: duplicate cancellation must not grow the global
slot registry, and nested Git helpers must preserve one request generation.

## Root causes

- `cancel_git_slot()` called `git_slot_begin()` even when the request registry
  had already dropped ownership. Repeated late cancels could therefore leave an
  empty slot entry behind indefinitely.
- `with_git_request_slot()` always opened a new generation. A composite Git
  operation entering a child helper could supersede its own parent generation,
  weakening cancellation and making multi-step work race-prone.

## Implementation

- `packages/ridge-core/src/commands/git.rs`
  - cancellation now atomically checks ownership before bumping a generation;
    unknown/duplicate cancels are no-ops and cannot allocate registry entries;
  - nested same-slot scopes reuse the ambient `(slot, generation)` and keep
    one cancellation identity through all child Git calls;
  - added deterministic tests for nested generation reuse and 128 duplicate
    unknown cancels.

## Verification

- `cargo test -p ridge-core commands::git::supersede_tests --lib --quiet`: 10/10
- `cargo test -p ridge-core commands::git --lib --quiet`: 39/39
- `pnpm check`: 0 errors, 0 warnings
- `git diff --check`: passed

## Residual gates

Physical phone attribution, WebView2 heap soak, public WebRTC, dual-window
workspace singleton, authenticated Git push, and full Kernel-domain migration
remain explicitly external or separately tracked; this slice does not claim
those gates complete.
