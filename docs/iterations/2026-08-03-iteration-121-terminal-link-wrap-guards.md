# Iteration 121 — installed terminal link wrap guards (2026-08-03)

## Scope

Close the remaining installed/Desktop terminal-link edge case after the
existing `06f5f74` fix. That earlier change already made Rust selection copy
join authoritative soft-wrapped rows and made the manager keep one complete
URL on every visual link segment. This iteration adds the boundary guard that
was still missing when punctuation was trimmed at a visual wrap edge.

## Change

- `packages/remote/src/shared/terminal/linkSpans.ts`
  - Continue a soft-wrapped span when the scanner trimmed only a
    punctuation-only suffix (`.,;:!?)\]}>`) from the visual row.
  - Restore that suffix into the logical target before appending the next row;
    the underline range still excludes the punctuation on the first segment.
  - Hard line breaks and links followed by ordinary text do not merge.
- `packages/remote/src/shared/terminal/linkSpans.test.ts`
  - Cover punctuation-at-wrap continuation.
  - Exercise the merged span through the hover decision, underline region,
    Ctrl-click decision, and host-open plan.
- `packages/ridge-term/src/selection.rs`
  - Add partial-selection coverage proving a wrapped URL copies without a
    visual newline. The selection implementation itself remains unchanged.

## Verification

- `pnpm exec vitest run packages/remote/src/shared/terminal/linkSpans.test.ts packages/remote/src/shared/terminal/linkAffordance.test.ts packages/remote/src/shared/terminal/linkOpenHost.test.ts --reporter=dot` — 23 passed.
- `cargo test -p ridge-term --lib --quiet` — 398 passed.
- `pnpm check` — 0 errors, 0 warnings.

Physical installed-WebView2 click/copy evidence is not available in this
environment; browser/build evidence cannot substitute for that device gate.
No version bump or publication was made because today's `v0.1.54` release
window is frozen.
