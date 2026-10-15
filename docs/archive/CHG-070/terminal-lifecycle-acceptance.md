# Runtime terminal lifecycle receipt

Migration evidence, not a CHG-070 completion certificate.

Natural PTY exit and ordinary-shell recovery now belong to `terminal_lifecycle.rs`. A bounded Runtime queue serializes decisions under the existing workspace lifecycle fence. The registry uses its owned child handle to observe exit every 100 ms; waiting for Windows ConPTY output EOF alone was insufficient. Epoch/channel fences prevent old readers or startup timers from changing a replacement's lifecycle. The reader retains a weak bridge reference, so retiring an old managed PTY releases its owned console resources rather than keeping them alive through the output task.

Only application-created, recognized ordinary shell launches with no explicit argv/environment qualify for automatic recovery. Explicit programs, tmux views and non-retired registered protocol members do not qualify. No Agent catalog/brand matcher or protocol session creation participates. The original pane/workspace identity remains; actual child PID/start instance and terminal epoch change. The canonical subscription follows the replacement snapshot. Old typed input fails its epoch check, and input to an exited process is rejected; stopped terminals retain a bounded read projection.

An initial shell may recover once. A replacement is disarmed until new user input succeeds; automation input, parser replies, snapshot recovery and resize do not re-arm it. This stops unattended startup/recovery loops. Explicit program argv and custom environments are never replayed. Existing scopes still bound recovery cwd, and failed recovery is reported in the close event without publishing another graph or claiming the old process survived.

The common shell creation path now selects `ridge-interactive`, restoring the existing PowerShell/bash/zsh prompt integration for all clients. Explicit respawn argv retain the previous unwrapped launch profile. Recovery preserves the original launch profile and shell program.

The frontend bridge's previous create/activate/default-shell decision is archived under `terminal-lifecycle/`. On a replacement event it only requests a fresh canonical stream/snapshot with a new presentation activation; it does not create a PTY, mutate a local VT fallback or choose a shell. Duplicate notices coalesce; a newer replacement arriving during snapshot recovery schedules one subsequent refresh.

Passing actual checks in `terminal_lifecycle_runtime.rs`:

- `runtime_rebuilds_ordinary_shell_without_presentation_and_fences_old_input`: no Native presentation during the first exit; same pane but new child/epoch; reopening and duplicate create cannot produce a second child; old input rejected, new input reaches the actual replacement; a subsequent user exit recovers; automation cannot start an unattended recovery loop; stopped snapshot remains readable and stopped input fails.
- `explicit_program_exits_without_automatic_replacement`: an explicit argv launch exits with no replacement and retains its stopped identity.
- `registered_protocol_member_prevents_ordinary_shell_rebuild`: a real existing protocol child runs inside an otherwise eligible ordinary shell, registers its actual descendant process/endpoint/session, and prevents shell recovery when the owned tree exits. Membership evidence remains. This is an owned fixture, not a user-started external Agent receipt.
- Windows-only `common_powershell_creation_and_rebuild_keep_real_prompt_integration`: the common Native-grant create command launches real PowerShell; both initial and replacement processes produce their actual OSC prompt notifications, with different terminal epochs.

Affected regressions passed: `terminal_events_runtime.rs` (one case); `owned_pty_process_tree.rs` (all three actual destroy/drop/service-shutdown cases); `commune_registration.rs` (existing process/conversation/permissions case); `outbound_hosts_runtime.rs` (authenticated TLS/foreign-view case). The ignored protocol fixture is executed as the real child and is not counted as a separate passing case. Frontend bridge tests cover projection-only recovery, duplicate/late notices and an overlapping recovery request.

Logs: `.webview2-dev-cdp/070-terminal-lifecycle-{runtime,events,owned-tree,commune,foreign}.log`; handoff logs use `070-terminal-lifecycle-final-{test,lint,typecheck,stc-verify}.log`. npm tests: 2,138 passed, 14 skipped, plus six Node tests; lint/typecheck: zero errors/warnings. Earlier failed exit/fixture attempts do not replace the final passing runs.

The initial receipt above did not cover rendered desktop/Remote behavior. The subsequent receipt below closes that particular gap.

## Rendered lifecycle and late-view receipt

`terminal-ui-acceptance.mjs` passed against owned debug desktop/Runtime copies and isolated profiles/data at `C:\Users\12867\AppData\Local\Temp\ridge-terminal-ui-7lyvMf\result.json`. Actual desktop keyboard input reached the initial and replacement CMD processes. The terminal epoch changed and the captured bridge log contains one recovery, without the second activation switch that had truncated typed commands. Canonical frames arriving during snapshot recovery are retained; stale frame callbacks and superseded recovery snapshots are fenced. Three deterministic bridge regressions cover these races.

The actual authenticated TLS Remote displayed the original terminal output. After an injected out-of-root recovery cwd and actual shell exit, both Native and Remote displayed the Runtime failure. Reloading Remote retained the actual `GUI_AFTER_REBUILD` output and exited state; a failed initial size claim no longer prevents canonical replay. Desktop failure colors were inspected in the screenshots. Force-closing and reopening the owned desktop retained the stopped PID and failure without creating another child. Screenshots and binary SHA-256 values accompany the receipt. The fixture uses real WebView2 keyboard/CDP and read-only terminal inspection; no personal Chrome/login profile or installed user instance participates.

`a_late_view_receives_actual_stopped_state_and_recovery_error` also passed: private stream metadata precedes its snapshot and includes the actual Runtime/terminal generation and retained error. Read-only activation succeeds without resizing or replacing the exited process, while activation in another workspace fails. Current affected Rust run: five lifecycle cases plus one event case passed; two child-fixture entries ignored as standalone tests. Log: `.webview2-dev-cdp/070-terminal-ui-rust.log`.

The five unproduced layout/global-focus/activity/circuit event consumers and their unused parser fixtures are archived under `frontend-runtime-events/`. Runtime pane-tree events retain forced fitting; Commune membership/task/session events retain roster/history refresh. The regenerated inventory lists 221 commands, 28 events and 56 computed sites, with no unresolved command or source-emitter gap. This is source attribution, not a certificate that every listed capability passed an end-to-end test.

The migration failure matrix and approved controller identity implementation now have receipts in migration-runtime-acceptance.md and controller-identity-acceptance.md. Actual NSIS Runtime exclusion is recorded in installer-runtime-acceptance.md. Final-build local performance passed 1,344 formal samples with zero timeouts/failures; see runtime-final-performance-acceptance.md. Still outstanding: real user-started external Agent desktop-kill acceptance and actual packaged product installation/upgrade.

Current handoff: `npm test` passed 2,124 Vitest cases (14 skipped) plus six Node cases; `npm run lint` and `npm run typecheck` each reported zero errors/warnings. The unused legacy event parser/circuit tests remain archived; bridge race and lifecycle projection tests remain active. Current logs: `.webview2-dev-cdp/070-identity-installer-{test,lint,typecheck,stc-verify}.log`. These checks do not mark CHG-070 complete.
