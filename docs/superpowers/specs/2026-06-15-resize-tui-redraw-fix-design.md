# Pane Resize TUI Redraw Fix — Refined Design

> Refines `.opencode/plans/2026-06-15-resize-tui-redraw-fix.md` after verifying its
> claims against the current code. Records what was kept, what was corrected, and
> what was implemented.

## Symptom

When an alt-screen / inline TUI (Claude Code without `CLAUDE_CODE_NO_FLICKER`,
lazygit, vim) is resized in wind, the screen shows offset rows/chars, partial
blanks, and stale cells from the old size ("错位行和字符 / 内容截断").

## Verification of the original plan

| Original claim | Verdict | Notes |
|---|---|---|
| 缺陷 1 — backend resize order reversed (PTY before wipe) | **Confirmed root cause** | Real regression; the mechanism is more precise than stated (see below). |
| 缺陷 2 — heuristic misses Claude Code | **Speculative, partially valid** | Reasoning is guesswork; kept a tightly-gated refinement, dropped the loose version. |
| 缺陷 3 — frontend `forceTuiWipe` plumbing | **Redundant** | `kernel.isInlineTuiMode()` routes through the same heuristic, so fixing 缺陷 2 covers it for free. |
| 修改 4 — second `forceFullRedraw` @ 800ms | **Dropped** | Non-root-cause band-aid; adds an unconditional repaint after every resize. |

### 缺陷 1 — the real mechanism

The invariant (documented at `manager.ts:4231-4243`): the §1.22 alt wipe / §A.3
inline-TUI wipe must land **before** the foreground TUI receives SIGWINCH, because
Ink/lazygit diff-renderers only re-emit cells that differ from their own
previous-frame model — cells wiped *after* a partial redraw stay blank.

Originally the frontend `fitPane` enforced this by calling `kernel.resize` (the
wipe) before `resizeHandler` (the PTY resize). The **P3.9.r / P4.4 refactor**
(`manager.ts:4282-4295`, `void wipeBeforePty`) moved the whole resize server-side
and delegated ordering to `resize_pane_inner` — which runs `master.resize()` (PTY →
SIGWINCH) at `terminal.rs:922` *before* `PaneParser::resize()` (the wipe) at
`terminal.rs:1030`. So the documented invariant was silently violated.

Why it actually misrenders: the wipe (`p.resize`) and the PTY reader thread share
the same `parser` mutex but in separate critical sections. On a **slow ConPTY
resize**, between `master.resize()` returning SIGWINCH and the resize thread
re-acquiring the parser lock to wipe, the reader thread can feed the child's
redraw bytes into the parser against the **stale** grid. The wipe then erases that
partial repaint, and Ink never refills it.

Confirmed the wipe genuinely lives in the backend path:
`PaneParser::resize` (`src-tauri/src/engine/parser.rs:242`) → `Terminal::resize`
(`packages/ridge-term/src/term/terminal.rs:546`) → heuristic →
`Grid::resize_with_inline_tui` (`grid.rs:640`, the §1.22/§A.3 wipe).

## Implementation

### 1. Backend reorder — `src-tauri/src/commands/terminal.rs::resize_pane_inner`

Refactored the inline resize body into two closures (`do_master_resize`,
`do_parser_resize`) and reordered by mode:

- **TUI** (`is_alt || is_inline_tui`): `do_parser_resize()` (wipe + emit Resize
  delta → mirror blanks) **then** `do_master_resize()` (SIGWINCH lands on blanks).
- **Shell**: `do_master_resize()` (SIGWINCH → PSReadLine/zsh-zle prompt redraw)
  **then** `do_parser_resize()` (§1.26 cursor-below cleanup). Unchanged behavior.

`master` and `parser` locks stay in separate scopes (no new lock-order hazard).
Stale §1.24/§A.3 comments that said the wipe "runs first via `manager.ts::fitPane`"
were corrected to "ran first above via `do_parser_resize`".

Minor benign change: a missing-workspace lookup now flows through the same
best-effort swallow path (logged via `pty_log::resize_err`, returns `Ok`) as the
already-swallowed `PaneNotFound`, instead of early-returning `Err` from the
command. Consistent with the design's "避免错误传播导致 session 中断" intent.

### 2. Refined heuristic — `packages/ridge-term/src/term/grid.rs`

Added `is_inline_tui_active_with_modes_at(now_ms, cursor_visible, app_cursor_keys,
mouse_reporting)`:

- Delegates to `is_inline_tui_active_at` first (all existing positives preserved).
- Fallback fires when a TUI mode (DECCKM `?1h` / mouse `?1000/?1002/?1003`) is on
  **and** there is a **recent absolute-positioning CSI** — so an inline TUI that
  keeps its cursor *visible* at the resize moment still gets the §A.3 wipe.
- Tightly gated: requires an **absolute** CSI (not a redraw-walk), and still
  short-circuits on alt screen and the Ctrl+C grace window. DECCKM/mouse are never
  set by line-editing shells (PSReadLine/zsh/fish), so no shell false-positives.

Wired `Terminal::resize` and `Terminal::is_inline_tui_mode_at`
(`terminal.rs:546/566`) to the new variant. Because `is_inline_tui_mode_at` backs
the wasm `isInlineTuiMode()`, this single change improves **both** the frontend
`isInlineTui` flag (→ reorder + skip-silence) and the backend wipe decision, with
no separate `forceTuiWipe` plumbing. `renderer.rs` stays on the base variant
(rendering gate doesn't need the broadened detection).

## Verification

- `cargo test -p ridge-term --lib` → **354 passed, 0 failed** (incl. 3 new
  heuristic tests; no regression from the shared `terminal.rs` change).
- `cargo check` on `src-tauri` → clean, **0 warnings**.

## Caveats / follow-up

- Backend (`src-tauri`) change needs a **ridge rebuild** to take effect; the wasm
  kernel (`packages/ridge-term`) needs rebuilding for the `isInlineTuiMode()`
  improvement to reach the frontend. Neither is verified at runtime yet.
- Runtime validation: open Claude Code (no `CLAUDE_CODE_NO_FLICKER`), drag the
  splitter repeatedly; with `localStorage.RIDGE_DIAG='1'` confirm
  `__RIDGE_KERNEL.lastResizeDiags()` shows `wipe_fired` / `inline_tui_wipe` true on
  resize.
