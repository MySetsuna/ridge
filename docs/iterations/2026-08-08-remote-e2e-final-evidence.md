# Remote / pane / workspace / NLM final evidence - 2026-08-08

## Root cause closed

The reproducible Explorer warning was not a filesystem or `get_file_tree` outage.
PTY metadata occasionally concatenated the next prompt marker to OSC 7 CWD:

```text
C:/code/wind/src-tauri\x07\x1b]133;A
```

`setPaneCwd` treated that value as a real path. The normalization boundary now
cuts a CWD at the first control byte, then applies the existing Windows/POSIX
canonicalization. Regression coverage includes the exact BEL + OSC 133 residue.

## Verification

- Focused Explorer/overlay regression tests: 49 passed; the full coverage run
  covered 155 files, 1606 passed, 1 skipped.
- Vitest v8 scope includes paneTree, fileExplorer, overlayScroll, and the remote
  artifact bundle: statements 53.81%, branches 39.84%, functions 62.45%, lines
  56.54%. Sonar new-code coverage is 80.2%.
- `pnpm check`: 0 errors, 0 warnings.
- `cargo fmt --all -- --check`: passed.
- `ridge-core` theme wire test: 1 passed.
- `ridge-cli` KernelHost tests: 12 passed.
- `cdp-multitab-freeze.mjs 3`: workspace probe 1 -> 2 -> 3, max lag 165 ms,
  long tasks 0, gate PASS, and no Explorer missing-path warning.
- LAN protocol, pane split/close broadcast, PTY parser, terminal input, and
  teammate E2E: passed; teammate 7/7.
- `rdg-remote-e2e.mjs`: desktop and mobile passed.
- `remote-leak-trace.mjs`: pane/workspace/reconnect/reap flow passed;
  `reap pass1=0`, `pass2=0`.
- `cdp-reap-test.mjs`: passed; the intentionally created orphan was reclaimed.

## Sonar boundary

Local SonarScanner is installed under
`.tools/sonar-scanner-8.0.1.6346-windows-x64/`; local Sonar server reports
`UP`. The earlier HTTP 401 came from scanner traffic using the configured
proxy; direct local authentication succeeds. Each scan used a short-lived
local token and revoked it afterward. Final scoped analysis uploaded
successfully, CE status `SUCCESS`, Quality Gate `OK`, `new_coverage=80.2%`,
`new_violations=0`, and duplicated-lines condition `OK`.

The analyzer still logs stale `.claude/worktrees` tsconfig parse warnings;
they do not block this explicit four-file analysis. Final log:
`.iteration/artifacts/sonar-authenticated-upload-final-coverage.log`.
Monitoring UI was not opened because the App Browser exposed no usable browser
target; API gate evidence is authoritative for this run.

Sonar-driven correctness fix: `fileExplorerStore.removeColumn` now chooses the
active column from the post-removal list, preventing a deleted first column from
remaining as `activeColumnId`; regression coverage locks this case.

## NLM workflow repair and recent pains

NLM authentication was refreshed through external Chrome CDP; MCP notebook and
recent-chat reads succeeded. Recent chats were used as read-only hypotheses:

- model stalls while searching/listing and exits on `max stall`;
- history/thinking/tool output is hard to scroll back and input hints obscure
  the workspace;
- pane/workspace identity, Remote readiness/reconnect, PTY/render continuity,
  geometry/DPR, Sonar/Coverage and Jules sidecar boundaries need evidence;
- transcript claims such as `REQ-007`/`REQ-008` being active are not local
  requirements evidence and remain `out_of_date_or_unverified`.

`notebooklm-iteration-loop/SKILL.md` and its workflow template now enforce:
read-only NLM by default, local evidence classification, no transcript-only
completion claims, and per-operation user authorization for NLM writes,
`git push`, releases, and Remote/Cloud activation.

After the final E2E pass, a second read-only NLM query produced five next-round
hypotheses: PTY fallback injection under concurrent output, DPR glyph shimmer,
cross-window background pane traffic, mobile background auth/E2EE eviction, and
Windows partial-cut permission desync. None has local confirmation; they are
recorded in `2026-08-08-nlm-next-iteration-hypotheses.md` with minimum evidence.

## Release gate

No release, Git push, Remote artifact upload, Cloud activation, or NotebookLM
source/note write was performed. Publication remains awaiting explicit user
authorization.
