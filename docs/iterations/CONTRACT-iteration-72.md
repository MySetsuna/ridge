# Iteration 72 Contract — Explorer partial cut recovery

- Date: 2026-07-29
- Status: approved / implementation
- Requirement: `REQ-EXPLORER-FILE-CONTINUITY-01`

## Delivery boundary

1. Batch paste continues reporting every failed source path and error.
2. A fully successful cut clears the internal clipboard.
3. A partially successful cut retains only failed source paths for retry.
4. A fully failed cut retains all source paths.
5. Copy clipboard behavior remains unchanged.

## Non-goals

- No filesystem transaction or rollback protocol.
- No overwrite of unsaved editor content.
- No host Ridge or real user-volume process.

## Deterministic gates

- Pure store tests cover full success, partial failure, full failure, ordering,
  and copy behavior.
- Focused Explorer store tests and Svelte diagnostics pass.
- `git diff --check`.
