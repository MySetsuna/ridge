# Runtime migration failure receipt

Evidence for the existing data namespace; not a CHG-070 completion certificate.

`registry.rs` now propagates filesystem inspection errors instead of treating them as absent state. Roster replacement validates the existing file and uses the existing archive-plus-atomic-write implementation. The original bytes, including legacy unknown fields, remain recoverable; repeated identical writes do not create another archive. Invalid or unreadable canonical data cannot be overwritten with an empty roster.

| Scenario | Actual evidence |
| --- | --- |
| Roster migration and restore | Original member and unknown fields recovered from the byte-identical archive; identical second save creates no new archive. |
| Damaged or incompatible roster | Both interrupted JSON and a missing required schema are rejected without changing or archiving the canonical bytes. |
| Windows denied access | A real OS share lock prevents roster replacement; after release the original bytes remain and retry succeeds. |
| Read-only legacy database | Real Runtime startup fails with `attempt to write a readonly database`; original DB bytes remain and no endpoint is published. |
| Partial startup failure | Database migration publishes its legacy archive, then corrupt workspace JSON prevents readiness. Repairing only the deliberately damaged fixture file lets the real Runtime reopen original project ID 71 and history row 72. First workspace mutation archives the exact legacy graph. |
| Locked roster at startup | Real Runtime fails on Windows error 32, preserves roster bytes and publishes no endpoint. Releasing the OS lock allows retry. |
| Repeat startup and restart | A second Runtime invocation retains the original running PID/start instance and changes none of the database, graph or roster bytes. Restart still reads original project ID 71 without repeating the committed database migration. |

Current logs: `.webview2-dev-cdp/070-migration-registry.log` (nine registry cases passed), `070-migration-runtime.log` (two actual Commune registration/retirement regressions passed), and `070-migration-runtime-failures.log` (the combined real Runtime failure/retry scenario passed). The final owned data directory is `C:\Users\12867\AppData\Local\Temp\ridge-partial-migration-45599987-cf3e-4fb9-973c-a8fcc703ef65`; its startup error logs and legacy archives remain available. No installed user data was migrated or modified.

Previously passing SQLite transaction interruption, archive restore, conflicting writer and schema rollback checks remain applicable: the unchanged `project_store.rs` and `.webview2-dev-cdp/070-project-migration-tests.log` were inspected, rather than rerunning that suite. The interrupted transaction is an owned child process fixture, not a forced power-loss test. Previous npm/lint/typecheck results are reused because no frontend input changed after those passing gates.

This proves one writer among current Runtime processes. Handoff from a still-running pre-070 desktop writer requires actual product installation/upgrade and remains unverified; this receipt does not claim otherwise. Controller private-key migration is approved and implemented; see controller-identity-acceptance.md. NSIS Runtime exclusion is covered by installer-runtime-acceptance.md. Packaged release GUI/install acceptance, the real user-started external Agent case and final-build performance measurement remain outside this receipt.
