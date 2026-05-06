# Repro: alt-screen resize wrap when running `claude` (§1.22 – §1.25)

This recipe walks through reproducing — and validating the staged §1.22→§1.25 fixes for — the bug where resizing a Ridge pane while a full-screen TUI (`claude`, `lazygit`, `vim`, etc.) is foreground produced visible auto-wrap / line-break artifacts and an off-row cursor on TUI exit.

## What the fix does

- **Kernel** (`packages/ridge-term/src/term/grid.rs`):
  - **§1.22** wipes the alt buffer on resize so SIGWINCH-driven redraws land on a blank canvas.
  - **§1.25** retires `reflow_primary` entirely — the kernel never rewraps cells across rows on resize. Both primary and alt screens always go through naive truncate/pad, eliminating the race between kernel-side cell migration and the TUI's own diff-driven redraw. (Subsumes §1.23: there is no longer a "reflow vs. naive" branch to choose between, so primary's `saved_cursor` is automatically anchored across resize.)
- **Backend** (`src-tauri/src/commands/terminal.rs`):
  - **§1.24** skips the ConPTY resize-silence window when the kernel is on alt screen. The 250 ms silence existed to suppress ConPTY's primary-viewport replay; on alt screen it has nothing legitimate to suppress and was actively dropping the TUI's own redraw bytes.
- **Frontend** (`src/lib/terminal/manager.ts::fitPane`):
  - **§1.25** swaps kernel-resize / PTY-resize ordering based on the kernel's alt-screen state. On alt screens the kernel resizes (and §1.22 wipes) BEFORE the PTY is told the new size, so the foreground TUI's SIGWINCH-driven redraw bytes always land on the freshly-cleared alt buffer. On primary screens the old PTY-first ordering is preserved (PSReadLine and other shells emit absolute cursor positions that must be interpreted under the new dimensions).

## Setup

1. **Build** with the §1.24 changes applied:

   ```bash
   pnpm tauri dev   # or `pnpm tauri build` for a release-mode repro
   ```

2. **Enable diagnostics** in the running app's devtools console (Ctrl+Shift+I in the Tauri window):

   ```js
   localStorage.RIDGE_DIAG = '1'         // expose __RIDGE_KERNEL on window
   localStorage.RIDGE_PTY_TRACE = '1'    // dump every PTY chunk to console
   location.reload()
   ```

   `RIDGE_PTY_TRACE` causes `manager.feed()` to log each PTY-to-wasm chunk as `[pty-trace][ts][pane8][NB] <hex>` so you can see the byte stream around a resize.

## Repro steps

1. Open a pane and run `claude` (or any Ink-based TUI). Wait for the welcome screen to fully render.
2. Drag the splitpane divider to shrink the pane width by ~50%.
3. Watch the alt screen redraw. **Before §1.24** you would see line-wrap artifacts and partial overlay; **after §1.24** the redraw should be clean.
4. Quit the TUI (`Ctrl+C` for `claude`, `:q!` for `vim`). The shell prompt should land on the original row, not several lines below.

## Verification commands (in devtools console)

```js
// Last 32 kernel resize calls — newest last.
__RIDGE_KERNEL.lastResizeDiags()
```

Expected fields per entry (post-§1.25):

```js
{
  old_rows, old_cols,         // dims before resize
  new_rows, new_cols,         // dims after resize
  is_alt: true,               // alt screen was active
  dim_changed: true,          // dims actually moved
  branch: "Naive",            // §1.25: only branch — kernel never reflows
  wipe_fired: true            // §1.22 wipe ran on alt buffer
}
```

`branch` collapsed to a single `Naive` variant in §1.25 because the kernel no longer rewraps cells across rows on resize; the `is_alt` and `wipe_fired` fields together carry the information the old `ReflowPrimary` / `NaivePrimary` / `NaiveOnly` triple used to encode.

If `is_alt: false` despite `claude` being on screen, the kernel parser missed a `?1049h` somewhere — that's a separate bug (and the §1.22 wipe naturally won't run).

If `wipe_fired: false` despite `is_alt: true`, the dim-change detection failed — check pixel-vs-cell rounding in the resize observer.

## Reading the PTY trace

After a resize, scroll back through the console looking for `[pty-trace]` lines. The timeline you're checking:

| Time | Expected |
|---|---|
| `t = 0` | Resize observer fires → manager calls `kernel.resize` → §1.22 wipe |
| `t = 0..250 ms` | Silence window (only when `is_alt: false`). With §1.24 active, **alt-screen panes skip this**, so PTY chunks should flow through immediately. |
| `t = ~tens of ms` | TUI's SIGWINCH-driven redraw chunks arrive. With §1.24 these reach the kernel; before §1.24 they were dropped if no shell-integration prompt OSC fired before the 250 ms hard timeout. |

If you see a multi-second gap with no `[pty-trace]` lines after the resize, the silence window is still engaged — either §1.24 isn't wired or the kernel reported `is_alt: false` to the backend.

## Cross-platform sanity

- **Windows**: ConPTY is the source of the silence-window mechanism, so this is the platform where the §1.24 fix matters.
- **macOS / Linux**: portable-pty's resize doesn't trigger any equivalent reflow byte storm, and the silence window is effectively a no-op there. The fix doesn't regress these platforms; the alt-skip path just exits the same code path slightly earlier.

## When to revisit

If this repro still shows wrap artifacts after the §1.25 fixes land:

1. Confirm `__RIDGE_KERNEL.lastResizeDiags()` reports `branch: "Naive"`, `is_alt: true`, `wipe_fired: true` (kernel side is correct — §1.22 wipe + §1.25 no-reflow contract).
2. Confirm the **frontend ordering swap** is in effect: on alt screens, `kernel.resize` must run BEFORE `entry.resizeHandler` invokes `resize_pane`. Add a temporary `console.debug('[ridge-term] resize order', { isAlt, ts: performance.now() })` around each call site in `manager.ts::fitPane` to verify the kernel-first path was taken when `isAlt === true`.
3. Confirm `[pty-trace]` chunks arrive within the first 50 ms of the resize (the §1.24 silence-window skip is still active).
4. If all three look right, the remaining suspect is the application's own redraw payload — Ink emits an incomplete diff that doesn't repaint every cell. Capture the literal redraw bytes claude emits (`localStorage.RIDGE_PTY_TRACE='1'`) for the first ~100 ms after the resize and confirm whether each visible cell is being touched or only a subset. If only a subset, the bug is upstream (Ink's diff against its previous-frame snapshot) and the kernel is correctly painting whatever it receives onto a clean canvas.
