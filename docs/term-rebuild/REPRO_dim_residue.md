# Repro: dim text & IME pre-edit residue (§1.27 — diagnostic phase)

This recipe is not a fix — it's an evidence-gathering harness. The user
reports two intertwined symptoms that share the colour "置灰" (greyed
text):

1. **"上一行污染" (previous-line pollution)** — greyed characters appear
   on a row that should hold fresh content, with no obvious source in
   the current command's output.
2. **"中文输入法预输入污染" (IME preedit pollution)** — Chinese-IME
   preedit characters seem to leave a faint residue on the canvas
   after `compositionend`, even though the textarea overlay has
   already been cleared.

Initial code review (§1.27 plan in
`~/.claude/plans/1-2-tui-3-resize-4-emoji-piped-floyd.md`) couldn't
disambiguate the two without live data. This doc walks through the
diagnostic instrumentation shipped with §1.27 Phase B and explains how
to capture logs that pin the root cause.

## Instrumentation shipped

- **`JsTerminal::cellsAt(row, col, len)`** (`packages/ridge-term/src/lib.rs`) —
  exposes per-cell `{ ch, codepoint, width, attrId, dim, bold, italic,
  underline, inverse, hidden, fg, bg }` for any range on the active
  screen. Returns `[]` for out-of-range rows. JS name: `cellsAt`.
- **IME composition trace** (`src/lib/components/RidgePane.svelte`) —
  `compositionstart` / `compositionupdate` / `compositionend` each emit
  `[ime] <event> { paneId, isComposing, imeValue, ... }` to the console
  when `localStorage.RIDGE_DIAG === '1'`.
- **Resize trace** (existing, from §1.22 / §1.24 / §1.26) —
  `__RIDGE_KERNEL.lastResizeDiags()` returns the last 32 resize
  decisions. Use this to correlate dim residue against a recent resize
  if the user dragged the pane during composition.

## Setup

1. Build with §1.27 Phase B applied:

   ```bash
   pnpm tauri dev    # or `pnpm tauri build`
   ```

2. Open a Ridge pane. Open the Tauri devtools (Ctrl+Shift+I).

3. Enable diag mode:

   ```js
   localStorage.RIDGE_DIAG = '1'
   location.reload()
   ```

4. Get a handle to the kernel for direct cell inspection. The wasm
   `JsTerminal` is held inside `TerminalManager` per pane; the
   simplest grab path:

   ```js
   // Dev-only window.__rt is set when import.meta.env.DEV is true
   // (TerminalManager singleton; manager.ts:249-250).
   const mgr = window.__rt;
   const paneId = /* the pane id you're inspecting */;
   const entry = mgr['entries'].get(paneId);   // private map; works in dev
   const kernel = entry?.kernel;
   ```

   Alternative: from a long-lived devtools snippet, watch
   `window.__rt._instance` and capture `entries` on first kernel
   creation.

## Branch A — "上一行污染" (previous-line pollution) repro

1. Run a command that writes DIM-attributed output. PowerShell with
   `oh-my-posh` themes that use SGR 2 (DIM) for the secondary path
   crumb — `paradox`, `agnoster` — already do this. Or force it:

   ```powershell
   "$([char]27)[2mDIM TEXT$([char]27)[0m`r`nplain text"
   ```

2. After the dim line scrolls past, run another command that produces
   normal output. Look for greyed characters on the new output's row.

3. While focus is in the pane, run in devtools:

   ```js
   // Cursor row at the moment of capture
   const r = kernel.cursorRow();
   kernel.cellsAt(r, 0, 80).filter(c => c.dim);
   ```

4. **Interpretation**:
   - Empty array → no DIM cells stored on the cursor row. The grey
     appearance is a **rendering artifact** (e.g. dirty-row diff
     missed a redraw, stale Canvas2D pixels). Look at
     `__RIDGE_KERNEL.lastResizeDiags()` for a recent resize.
   - Non-empty with `dim: true` cells → grid state genuinely holds
     DIM-attributed cells. Either an app emitted them and the kernel
     correctly stored them (no bug), OR the kernel's `erase_in_*`
     paths are leaking SGR onto cleared cells. Inspect the `attrId`
     and `bg` fields — `attrId` should be 0 (DEFAULT) for "blank"
     cells; non-zero `attrId` with `dim: true` means the erase path
     wrote a non-default attr.

5. To narrow the source: run a wider scan immediately after the
   suspected leak:

   ```js
   for (let r = 0; r < kernel.rows(); r++) {
     const dim = kernel.cellsAt(r, 0, kernel.cols()).filter(c => c.dim);
     if (dim.length) console.log('row', r, dim);
   }
   ```

   Compare the rows containing DIM cells against the visible content.

## Branch B — IME preedit residue repro

1. Switch to Chinese IME (Microsoft Pinyin or similar).

2. Click into the pane to focus it.

3. Type a long pinyin string (e.g. `nihao` for 你好) but DO NOT commit
   yet. Watch the textarea overlay grow over the canvas.

4. Press Esc to cancel composition (or keep typing and press Enter to
   commit a candidate).

5. Devtools should show:

   ```
   [ime] start { paneId: ..., isComposing: false, imeValue: '' }
   [ime] update { paneId: ..., isComposing: true, dataLen: 5, data: 'nihao' }
   [ime] end { paneId: ..., isComposing: false, imeValue: '', committed: '' or '你好' }
   ```

6. Right after `compositionend`, capture cells around the cursor:

   ```js
   const r = kernel.cursorRow();
   const c = kernel.cursorCol();
   kernel.cellsAt(r, Math.max(0, c - 10), 30);
   ```

7. **Interpretation**:
   - Cells `ch` matches what the user expected at those positions
     (e.g. spaces left of cursor, the committed Chinese chars at
     cursor-2..cursor) and `dim: false` everywhere → grid state is
     correct. The visible residue is **CSS / textarea overlay leak**:
     the textarea was opaque while composing and shrank back to
     `opacity: 0` on `compositionend`, but the canvas underneath
     wasn't redrawn for those cells. Fix likely lies in invalidating
     the renderer's per-row hash for the cursor row on
     `compositionend`.
   - Some cells have `dim: true` or unexpected `ch` → the
     composition path is somehow writing into the grid (which it
     shouldn't — only the textarea is supposed to render preedit
     text, and the committed bytes go through `manager.write()` →
     PTY → echo → `Grid::print`). Trace the source.

## Reporting

Paste the captured logs back into the PR / commit that wires up
the §1.27 fix. Include:

- `[ime] start/update/end` lines for the failing composition.
- `cellsAt` snapshot of the affected rows (DIM ones) at the moment
  the residue is visible.
- A screenshot of the residue if possible.
- `__RIDGE_KERNEL.lastResizeDiags()` output if a resize happened
  near the symptom.

## Companion docs

- `REPRO_alt_resize.md` — alt-screen TUI resize bug (§1.22 / §1.24).
- `REPRO_primary_resize.md` — primary-screen prompt residue
  (§1.26 / §A.2 / §A.3).
- This file — dim-text / IME residue (§1.27, evidence-gathering only).
