# Wave 83: Sonar stale Quality Gate handoff (2026-08-11)

## Finding

The Ridge project card is still showing the result of analysis
`c271e74b-ac3f-4277-bbef-74418f48b822`. That analysis completed in SonarQube,
but its Quality Gate was `ERROR` because `new_violations=1` while the gate
requires `0`. The same result recorded `new_coverage=80.1` and
`new_duplicated_lines_density=0.84181`, both passing their thresholds.

The card-level `80.5%` coverage and `2.6%` duplication values therefore do not
prove that the Gate passed: the Gate evaluates its configured conditions, and
the card is not automatically refreshed by a local source change or a Git
commit.

## Current source checks

- `packages/ridge-cli/src/tui/lan_host_impl.rs:589` is now the low-complexity
  `subscription_field` helper. The former `rust:S3776` body was split into
  focused helpers in commit `254a98e3`.
- `src/remote/index.html:4` contains
  `<title>Ridge Remote - Agent Terminal</title>`, addressing the historical
  `PageWithoutTitleCheck` issue.
- The related Rust tests, Cloud focused tests, full Vitest suite, `pnpm check`,
  script syntax checks, remote builds, desktop build, main build, and PWA
  verifier are recorded as green in Wave 80 and Wave 82.

## Sonar server evidence

- Local server: `http://127.0.0.1:9000`, version `26.7.0.124771`, system status
  `UP`.
- Authenticated Quality Gate and issue APIs currently return HTTP `401`.
- `/api/authentication/validate` reports `valid=false` for the available
  administrator credentials. No credential reset or token guessing was done.
- Read-only Elasticsearch inspection still lists the two open issues from the
  previous analysis: the historical HTML title issue and the old Rust
  cognitive-complexity issue. This is stale server state, not evidence that the
  current worktree still contains those defects.

## Required closeout

After valid Sonar authentication is restored, run a fresh scan for project
`MySetsuna_ridge` with the repository's intended source scope, wait for the CE
task, and record all of the following:

1. scanner and CE task success;
2. project coverage at least `80.0%`;
3. Quality Gate `OK` with `new_violations=0`;
4. no open current issues for `rust:S3776` or `Web:PageWithoutTitleCheck`;
5. the sanitized scanner log, measures, issue result, and failure cause if any.

Until that scan succeeds, `REQ-SONAR-COVERAGE-80-01` remains `ACTIVE`; local
tests and LCOV must not be presented as a substitute for the Sonar project
metric or Quality Gate.

## Scanner runtime prerequisite

Several later local attempts stopped before upload because the Sonar JS/TS
bridge could not start or became unresponsive. The scanner selected the
bundled Node `v24.11.0`, while the interactive toolchain exposes `v25.9.0`;
both paths are unsuitable to assume without a scanner compatibility check.
Node `v20.19.0` is installed locally and is the candidate for the next
controlled scan:

```powershell
sonar-scanner.bat -Dsonar.nodejs.executable=C:\DevKit\nvm\v20.19.0\node.exe -Dsonar.qualitygate.wait=true
```

The command still requires a valid scanner token. The scan must be run with a
bounded process set and its exit/CE result recorded; do not treat a report that
fails in the JS bridge as an uploaded analysis.

## Latest local regression

- `ab9376ef` removes an `unused_mut` warning in `history_overlay_geometry`;
  `cargo test -p ridge-term --lib` passed `399/399`.
- `b80fe50c` fences Remote worker raw `feed` and `applyDelta` through one
  monotonic per-pane `renderFrameId`; the focused worker suite passed
  `4 files / 99 tests`.
- `pnpm check` passed with `0 errors / 0 warnings`; full
  `pnpm test:coverage:sonar` exited `0`. The local LCOV summary is
  statements `66.02%`, branches `60.21%`, functions `68.23%`, lines `70.11%`;
  it remains supporting evidence only and does not replace a Sonar project
  scan.
