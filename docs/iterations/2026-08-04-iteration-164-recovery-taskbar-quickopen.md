# Iteration 164 — Desktop recovery, taskbar Jump List, and path-aware Quick Open

## Scope

Close the verified desktop regressions from the previous release: WebGPU
workspace switching could fan out renderer creation, restart restore could
reacquire the wrong workspace, surviving kernel PTYs could race a cold Pane
mount, and Remote Host topology loading could remain pending indefinitely.
This round also closes the approved taskbar, Quick Open, and drag/drop focus
requirements.

## Change

- Serialize shared WebGPU `RenderHandle` creation, latest-wins active-workspace
  invalidation, and memory-pane restoration. The startup gate now waits for
  `reattach_kernel_ptys` before a desktop Pane can create a replacement shell.
- Preserve the backend-selected workspace returned by `.ridge` restore/open;
  stale desktop window acquisition no longer steals the active tab.
- Bound each Remote Host pane-topology request at 15 seconds, retain the last
  successful tree on failure, and prevent stale connection attempts from
  overwriting a newer loading/error banner.
- Add a Windows taskbar Jump List with a `最近关闭的工作区` category and an
  `打开新窗口` task. Recent entries pass a validated `.ridge` path through
  both cold-start and single-instance activation paths.
- Make `filename_search` match root-relative path fragments (including partial
  directory prefixes), keep basename/path relevance ordering, and discard
  stale Quick Open results during rapid typing.
- Focus the exact terminal Pane after both native OS file drops and Explorer
  drag/drop path pastes.

## Verification

- `pnpm check`: passed, 0 errors / 0 warnings.
- Focused Vitest: 5 files, 99 tests passed (WebGPU lifecycle, workspace
  restore, Remote Host timeout/progress, drag/drop focus, terminal memory).
- `cargo test -p ridge-core`: 316 tests + 1 doctest passed.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib taskbar::tests::parses_workspace_arg_forms`: passed.
- `cargo check --manifest-path src-tauri/Cargo.toml`: passed; existing
  warnings only.
- `git diff --check`: passed. No version bump or release is made.

## Remaining external gates

Physical WebGPU heap soak, Windows taskbar visual activation, dual-window
workspace singleton behavior, and public Remote Host attach still need their
respective runtime environments; source-level changes do not claim those
external results.
