# Iteration 138 - tab switching and desktop PTY reattach

## Scope

Close the approved desktop restart gap while finishing the tab-switch black
frame fix. The kernel process is the lifecycle owner for ordinary shell PTYs;
Tauri remains the UI/parser proxy. Structured Agent launches keep the existing
local path because their per-process environment and teammate/TMUX contract is
not interchangeable with a plain shell.

## Root causes

- Workspace switching invalidated the shared WebGPU host while inactive renderers
  were still being asynchronously unparked. The host could clear between the
  switch and the first draw, producing a black frame.
- Tray `退出桌面端` already left `ridge-kernel` alive, but ordinary desktop PTYs
  were still owned by `AppState::Workspace.terminals` and their local child
  handles. Once Tauri exited, no stable endpoint or pane-to-PTY key remained.

## Changes

- `TerminalManager` batches host invalidation during memory-park restore and
  paints only if the active workspace generation is still current.
- Workspace switching activates a cached tree immediately, ignores stale IPC
  replies, and registers CWD listeners against the target tree in parallel.
- `ridge-kernel` domain PTY creation accepts a stable `pty_id` (the pane UUID),
  workspace metadata, and exposes authenticated PTY discovery. Output leases
  remain bounded and report sequence metadata for reconnect cursors.
- Tauri implements a `portable_pty` master/writer proxy over the kernel's
  write/resize/clear/destroy and bounded output-lease endpoints. Existing
  parser, scrollback, delta and Pane lifecycle code remains shared.
- Ordinary shell creation uses the kernel when available; explicit structured
  Agent launches retain the local path. Pane close/replacement explicitly
  destroys kernel PTYs, while dropping a desktop proxy does not.
- If the first pane wins the asynchronous kernel bootstrap race, the ordinary
  shell path enters the same singleton lifecycle helper before falling back;
  unit tests remain local and never spawn an external kernel.
- Startup invokes `reattach_kernel_ptys` after workspace restore. Both explicitly
  saved `.ridge` files and unsaved tabs are represented in the close-time restore
  manifest (unsaved tabs use private `session-workspaces/session-<workspace>.ridge`
  snapshots). The same pane UUID is found across a restored workspace (even if
  the workspace UUID was regenerated), the proxy resumes after the kernel's
  latest output sequence, and the PTY process stays alive.
- Kernel PTY creation carries the `ridge-interactive` launch profile. PowerShell,
  Bash, and Zsh emit the same OSC 7/OSC 133 markers as the local desktop path,
  preserving live CWD/PaneHeader updates after a reconnect.
- Kernel-owned scrollback is used as the desktop tail fallback, so a restarted
  desktop can paint retained output before new bytes arrive.

## Verification

- `pnpm check` - 0 errors, 0 warnings.
- Full Vitest - 147 files, 1524 passed, 1 skipped (last TypeScript change set).
- `cargo test -p ridge-kernel --lib` - 31 passed, including stable pane
  identity/list discovery.
- `cargo test -p ridge --lib` - 252 passed.
- `cargo check -p ridge-kernel -p ridge --lib` - exit 0; existing unrelated
  dead-code warnings remain.
- `git diff --check` - passed.
- Commit `f0ed195` pushed to `origin/main`; it adds the interactive shell launch
  profile and private unsaved-workspace session snapshots.

## Acceptance and limits

The code path now provides real reattach for ordinary shell PTYs created after
this change and preserves the explicit full-exit menu as the kernel destroy
boundary. PTYs created by older builds were local and cannot be retroactively
transferred after their ConPTY master was dropped; they are not falsely claimed
as recoverable. Physical tray-exit/restart, WebView2 memory soak, and public
Remote/Cloud deployment remain external runtime gates. No version bump or
release was made; today's `v0.1.54` publication allowance remains consumed.
