# Repro: primary-screen resize residue (§1.26 + §A.2 + §A.3)

This recipe reproduces — and validates the §1.26 / §A.2 / §A.3 fixes for
— the bug where resizing a Ridge pane on a **primary** screen (typical
PowerShell + oh-my-posh prompt, OR an Ink-based inline TUI like Claude
Code's input box) leaves ghost characters past the new prompt's end and
collapses the path-to-`>` gap.

Companion to `REPRO_alt_resize.md` (which covers the alt-screen path).

## Symptom

Before the fix:

```
# Wide window — prompt fits comfortably:
PS C:\code\wind > █

# Drag pane narrower → ghost residue + collapsed gap:
PS C:\code\wind>e█    <- '>' glued to path; trailing 'e' from old prompt
```

The `>` symbol normally has a single space gap from the path. After a
resize that gap collapses and one or more characters from the previous
(wider) prompt linger on the same row past the new prompt's end.

For Ink-based inline TUIs (Claude Code's input box, lazygit running on
primary), the broken-up box border and wrapped status text appear on
rows ABOVE the cursor row instead of past the cursor — that case
needs a wider wipe than the §1.26 cursor-row+below partial cleanup.

## Root cause (combined)

1. **`grid.rs::resize` did no post-naive cleanup on primary screens.**
   §1.25's naive truncate/pad only adjusts row vector lengths; it never
   blanks cells the new prompt won't repaint. Old prompt text past the
   new prompt's end stayed on screen.

2. **`engine/pty.rs::RESIZE_SILENCE_WINDOW_MS` was 250 ms.** The window
   exists to swallow ConPTY's viewport replay, but PSReadLine /
   fish-zle / zsh-zle SIGWINCH-driven prompt redraws also arrive
   inside that window (typical 10–50 ms post-SIGWINCH) and were
   silently dropped, so the redraw never landed on the kernel grid.

3. **`renderer.rs::tick` only resized its dirty-row snapshot on
   *growth*.** On a *narrowing* primary resize the snapshot was still
   sized to the old grid, so Canvas2D's per-row diff never marked the
   trailing rows for redraw — even after kernel cells were correct,
   stale pixels stayed on the canvas.

## What the fix does

- **§1.26 partial cleanup** (`grid.rs::resize_with_inline_tui`, gate
  `dim_changed && !is_alt && !inline_tui_active`): blank
  `cursor_row[cursor.col + 1 ..]` and every row strictly below the
  cursor. Cells AT cursor.col and to its left are preserved (raw
  shells without SIGWINCH-driven full redraws keep their in-progress
  text). Rows above the cursor are scrollback / prior command output
  and are never touched. Surfaced in `ResizeDiag.cleared_below_cursor`.
- **§A.3 full inline-TUI wipe** (`grid.rs::resize_with_inline_tui`,
  gate `dim_changed && !is_alt && inline_tui_active`): wipe the entire
  visible primary region + home cursor + reset scroll region. Used
  when the caller flagged an Ink-style app as currently foregrounded.
  Mutually exclusive with the partial cleanup (full wipe makes the
  partial one redundant). Surfaced in `ResizeDiag.inline_tui_wipe`.
  The heuristic itself (`Grid::is_inline_tui_active_at`) is computed
  from `cursor_visible == false` + a 2-second decay window since the
  last absolute-positioning CSI (`CUP H`, `HVP f`, `CHA G`, `HPA \``,
  `VPA d`).
- **§A.2** (`engine/pty.rs::RESIZE_SILENCE_WINDOW_MS`): 250 ms → 80 ms.
  Still suppresses ConPTY's viewport replay tail but lets PSReadLine's
  legitimate redraw through.
- **§A.3 renderer side** (`render/renderer.rs::tick`): the snapshot
  resize branch fires on *any* row-count change, not only growth.
  Pairs with §1.26 by ensuring the next frame re-hashes every row
  against the cleared cells.

## Setup

1. Build with the §1.26 / §A.2 / §A.3 changes applied:

   ```bash
   pnpm tauri dev    # or `pnpm tauri build` for a release-mode repro
   ```

2. Open a Ridge pane running PowerShell with an `oh-my-posh` theme that
   includes a path-to-`>` separator. Any of the bundled themes work
   (`paradox`, `agnoster`, `jandedobbeleer`). If you don't have one
   active:

   ```powershell
   oh-my-posh init pwsh --config "$env:POSH_THEMES_PATH\paradox.omp.json" | Invoke-Expression
   ```

3. Enable diagnostics in the Tauri devtools (Ctrl+Shift+I):

   ```js
   localStorage.RIDGE_DIAG = '1'
   location.reload()
   ```

## Repro steps — plain shell (§1.26 partial cleanup)

1. Make sure the pane is wide (≥120 cols). Confirm the prompt looks
   like `<short_path> > ` with a clear space before `>`.
2. Drag the splitpane divider to shrink the pane to roughly 70 cols.
   Watch the prompt repaint.
3. **Before the fix**: the path collapses against `>` and a few stray
   characters from the wider prompt remain past the new `>`. Repeat
   the resize a few times to make the residue obvious.
4. **After the fix**: the prompt repaints cleanly with the path-to-`>`
   gap intact and no trailing characters past `>`.
5. Drag the pane back to wide (~120 cols). The new prompt should
   re-emit at the new width with no leftover narrow-mode characters
   wrapping into the row above.

## Repro steps — Ink-based inline TUI (§A.3 full wipe)

1. Start `claude` in a primary pane (no `?1049h` — Claude Code's input
   box draws inline on primary). Wait for the input box border to
   render.
2. Drag the splitpane divider to shrink the pane width.
3. **Before §A.3**: the input box's top/side borders stay drawn at the
   old width, leaving a wrapped fragment on the row(s) above the new
   border. The cursor row's right side is clean (§1.26 fired) but the
   rows ABOVE still show stale border characters.
4. **After §A.3**: the entire visible region wipes; Ink's diff redraw
   on SIGWINCH paints the new layout against blanks and the box
   border is intact at the new width.

## Verification

In the Tauri devtools:

```js
// Last 32 kernel resize calls — newest last.
__RIDGE_KERNEL.lastResizeDiags()
```

For a **plain-shell** primary resize you should see entries like:

```js
{
  old_rows: 32, old_cols: 120,
  new_rows: 32, new_cols: 70,
  is_alt: false,
  dim_changed: true,
  branch: 'Naive',
  wipe_fired: false,            // alt-screen wipe did NOT run
  cleared_below_cursor: true,   // §1.26 partial cleanup ran
  inline_tui_wipe: false,       // §A.3 full wipe did NOT run
  inline_tui_active: false      // heuristic was off
}
```

For an **inline-TUI** primary resize (Ink foreground):

```js
{
  ...
  is_alt: false,
  cleared_below_cursor: false,  // skipped — full wipe is more thorough
  inline_tui_wipe: true,        // §A.3 full wipe ran
  inline_tui_active: true       // heuristic detected Ink (cursor hidden + recent abs CSI)
}
```

Mutual-exclusion invariants you can check:
- At most one of `wipe_fired`, `inline_tui_wipe`, `cleared_below_cursor`
  is `true` per resize.
- `wipe_fired` requires `is_alt == true`; the other two require
  `is_alt == false`.

## Cargo regression coverage

```bash
cargo test --manifest-path packages/ridge-term/Cargo.toml \
  --test protocol_smoke -- scenario_primary_resize
```

Runs:
- `scenario_primary_resize_clears_below_cursor` — verifies cells past
  the cursor on the cursor row, plus all rows below, are blanked when
  no inline-TUI flag is set.
- `scenario_primary_resize_preserves_left_and_above_of_cursor` —
  verifies cells left-of-cursor on the cursor row, plus all rows
  above, survive the resize.

## Companion: alt-screen path

This document only covers primary screens. For the alt-screen
counterpart (running `claude` after `?1049h`, or `lazygit` / `vim`)
see `REPRO_alt_resize.md` — that path uses the §1.22 alt wipe +
§1.24 silence-skip and is mutually exclusive with §1.26 / §A.3 (alt
panes never go through the primary cleanup paths).
