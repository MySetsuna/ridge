/**
 * §1.32 (2026-05-20) Wave F — PTY-prompt suffix snapshot.
 *
 * The keystroke-mirror model (Wave A-E) tracks what the user typed by
 * intercepting each key event. That's fast and reactive, but it can't
 * see anything the SHELL does on its own: Tab completion echoes,
 * Ctrl+R reverse-search redraws, $VAR expansion, alias substitution,
 * vi-mode movement. Each of those silently drifts the mirror away
 * from the real shell line, and the `\x08` replay sends the wrong
 * number of backspaces — leaving garbage on the line after the user
 * picks a history entry.
 *
 * Wave F's answer: don't model the shell. Just READ THE LINE.
 *
 * The kernel already has `cellsAt(row, col, len)` which returns the
 * grid contents. Combined with `cursorRow()` / `cursorCol()` and a
 * pane-level "where did this input start" marker, we can reconstruct
 * the exact shell-input string at any moment — no keystroke
 * accounting needed.
 *
 * This file holds the PURE reconstruction function. The kernel
 * reads and pane-level state live in `manager.ts`.
 */

/** Minimal shape we read out of `kernel.cellsAt()`. The real return
 *  type has many more fields (attrs, codepoint, etc.) but we only
 *  need the rendered character and the column-width. */
export interface CellLike {
	readonly ch: string;
	readonly width: number;
}

/** Snapshot of the typed shell-input. `text` is the concatenated
 *  rendered characters from the prompt-end to (the trimmed) end of
 *  line. `cursorCol` is the cursor's position INSIDE `text` measured
 *  in UTF-16 code units. */
export interface ShellInputSnapshot {
	readonly text: string;
	readonly cursorCol: number;
}

/**
 * Reconstruct the shell-input string from two cell ranges read from
 * the kernel grid:
 *   - `preCursorCells`: cells from `inputStartCol` up to (but not
 *     including) `cursorCol` — i.e. text BEFORE the cursor.
 *   - `postCursorCells`: cells from `cursorCol` to end-of-row — i.e.
 *     text AFTER the cursor, plus the row's trailing blank fill.
 *
 * Algorithm:
 *   - Each cell contributes `cell.ch` to the result string. Cells
 *     with `width === 0` (continuation cells of wide-char cells)
 *     are skipped — the leading wide-char cell already provided
 *     the full character.
 *   - Pre-cursor text is preserved verbatim (including any trailing
 *     whitespace the user genuinely typed).
 *   - Post-cursor text has its trailing whitespace trimmed (those
 *     blanks are the row's end-of-line fill, not typed content).
 *
 * Result:
 *   - `text` = pre-cursor text + trimmed post-cursor text.
 *   - `cursorCol` = pre-cursor text length in UTF-16 code units.
 *
 * Pure: no DOM access, no kernel access. Test-friendly.
 */
export function reconstructInputSnapshot(
	preCursorCells: readonly CellLike[],
	postCursorCells: readonly CellLike[],
): ShellInputSnapshot {
	let textBefore = '';
	for (const c of preCursorCells) {
		if (c.width === 0) continue;
		textBefore += c.ch;
	}
	let textAfter = '';
	for (const c of postCursorCells) {
		if (c.width === 0) continue;
		textAfter += c.ch;
	}
	// Trim only the post-cursor blank tail. The pre-cursor segment is
	// preserved as-is so trailing whitespace the user actually typed
	// (e.g., "ls   |") survives.
	const trimmedAfter = textAfter.replace(/\s+$/, '');
	return { text: textBefore + trimmedAfter, cursorCol: textBefore.length };
}
