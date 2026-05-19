/**
 * §1.32 (2026-05-20) — pure state machine for the shell-input mirror
 * that the history-popup uses as its `query` and as the `\x08` replay
 * count when the user picks a history entry.
 *
 * Wave C upgrade: state is now `{ text, cursorCol }`, not a flat
 * string, so ArrowLeft / ArrowRight / Home / End / Delete each map
 * to a precise event rather than the "invalidate the mirror" clear
 * approximation used in Wave B. This makes mid-line edits survive a
 * subsequent ArrowUp / history pick.
 *
 * Why a separate module: same as before — every keystroke path that
 * mutates the mirror is a chance to drift from the shell's actual
 * line. Extracting the rules lets us:
 *
 *   1. Test each event without DOM / Svelte plumbing.
 *   2. Fix one event at a time without touching the component.
 *   3. Mark deferred events with `it.todo` against the same module
 *      so future work has a concrete promotion target.
 *
 * Current coverage (Wave C):
 *   - char insertion at the cursor
 *   - backspace (deletes char BEFORE cursor)
 *   - delete (deletes char AT cursor — Wave B used to `clear` here)
 *   - arrowLeft / arrowRight / home / end — cursor movement, text unchanged
 *   - paste (inserts at cursor, advances cursor)
 *   - Ctrl+U kill-line     → empty state, cursor 0
 *   - Ctrl+W kill-word     → drops trailing word *before* the cursor,
 *                             preserves the suffix *after* the cursor
 *   - Ctrl+K kill-to-eol   → keeps prefix up to cursor, drops the rest
 *   - Enter / Escape / explicit reset → empty state, cursor 0
 *
 * Deferred (still `it.todo`):
 *   - Tab completion echo sync (Wave E)
 *   - shell-line length validation before `\x08` replay (Wave D)
 *   - PTY-prompt-suffix snapshot as buffer source-of-truth (Wave F)
 */

/** Mirror of the shell input line. `cursorCol` is a UTF-16 code-unit
 *  offset into `text`, in the range `[0, text.length]` inclusive
 *  (= text.length means "cursor is at end of line"). Treat this
 *  struct as immutable — every event returns a fresh value. */
export interface InputBufferState {
	readonly text: string;
	readonly cursorCol: number;
}

/** Initial / reset state. Same reference is reused for `clear` /
 *  `killLine`; identity comparison is safe for "is this empty?". */
export const EMPTY_INPUT_BUFFER: InputBufferState = Object.freeze({ text: '', cursorCol: 0 });

/** Events the buffer state machine understands. */
export type InputBufferEvent =
	| { type: 'char'; char: string }
	| { type: 'backspace' }
	| { type: 'delete' }
	| { type: 'arrowLeft' }
	| { type: 'arrowRight' }
	| { type: 'home' }
	| { type: 'end' }
	| { type: 'killLine' }
	| { type: 'killWord' }
	| { type: 'killToEol' }
	| { type: 'paste'; text: string }
	| { type: 'clear' };

/** Structural subset of `KeyboardEvent` — the only fields
 *  `deriveBufferEvent` reads. DOM `KeyboardEvent` instances satisfy
 *  this shape directly, so callers pass `e` without conversion. */
export interface KeySpec {
	readonly key: string;
	readonly ctrlKey: boolean;
	readonly metaKey: boolean;
	readonly altKey: boolean;
	readonly shiftKey: boolean;
}

/**
 * Map a DOM keydown event to an `InputBufferEvent`, or `null` if the
 * key doesn't affect the mirror (modifier-only press, function keys
 * we don't model, etc.). Pure function — no DOM access.
 */
export function deriveBufferEvent(spec: KeySpec): InputBufferEvent | null {
	// Readline kills — Ctrl-prefixed letter, no other modifiers.
	if (spec.ctrlKey && !spec.metaKey && !spec.altKey && !spec.shiftKey) {
		if (spec.key === 'u' || spec.key === 'U') return { type: 'killLine' };
		if (spec.key === 'w' || spec.key === 'W') return { type: 'killWord' };
		if (spec.key === 'k' || spec.key === 'K') return { type: 'killToEol' };
	}
	// Printable single-character key without Ctrl / Meta. Shift is
	// allowed (Shift+a → 'A'); Alt is allowed (Alt+a on macOS emits
	// dead-key chars that we still want to mirror).
	if (spec.key.length === 1 && !spec.ctrlKey && !spec.metaKey) {
		return { type: 'char', char: spec.key };
	}
	if (spec.key === 'Backspace') return { type: 'backspace' };
	if (spec.key === 'Delete') return { type: 'delete' };
	if (spec.key === 'ArrowLeft') return { type: 'arrowLeft' };
	if (spec.key === 'ArrowRight') return { type: 'arrowRight' };
	if (spec.key === 'Home') return { type: 'home' };
	if (spec.key === 'End') return { type: 'end' };
	if (spec.key === 'Enter') return { type: 'clear' };
	return null;
}

/**
 * Apply an event to the current buffer state and return the new
 * state. Pure: input is not mutated.
 *
 * If `state.cursorCol` is out of range (corrupted / pre-Wave-C
 * legacy), it gets clamped to `[0, text.length]` defensively before
 * the event is applied.
 */
export function updateInputBuffer(
	state: InputBufferState,
	ev: InputBufferEvent
): InputBufferState {
	const { text } = state;
	const col = clamp(state.cursorCol, 0, text.length);
	switch (ev.type) {
		case 'char': {
			const newText = text.slice(0, col) + ev.char + text.slice(col);
			return { text: newText, cursorCol: col + ev.char.length };
		}
		case 'backspace': {
			if (col === 0) return col === state.cursorCol ? state : { text, cursorCol: col };
			return { text: text.slice(0, col - 1) + text.slice(col), cursorCol: col - 1 };
		}
		case 'delete': {
			if (col >= text.length) return col === state.cursorCol ? state : { text, cursorCol: col };
			return { text: text.slice(0, col) + text.slice(col + 1), cursorCol: col };
		}
		case 'arrowLeft':
			if (col === 0) return col === state.cursorCol ? state : { text, cursorCol: col };
			return { text, cursorCol: col - 1 };
		case 'arrowRight':
			if (col >= text.length) return col === state.cursorCol ? state : { text, cursorCol: col };
			return { text, cursorCol: col + 1 };
		case 'home':
			return col === 0 && col === state.cursorCol ? state : { text, cursorCol: 0 };
		case 'end':
			return col === text.length && col === state.cursorCol ? state : { text, cursorCol: text.length };
		case 'killLine':
			return EMPTY_INPUT_BUFFER;
		case 'killWord':
			return killWordAtCursor(text, col);
		case 'killToEol':
			return { text: text.slice(0, col), cursorCol: col };
		case 'paste': {
			const newText = text.slice(0, col) + ev.text + text.slice(col);
			return { text: newText, cursorCol: col + ev.text.length };
		}
		case 'clear':
			return EMPTY_INPUT_BUFFER;
	}
}

/**
 * Drop the trailing word *before the cursor* (matching GNU readline's
 * `unix-word-rubout` Ctrl+W), preserving whatever text was already
 * past the cursor. Cursor lands where the kill left off.
 */
function killWordAtCursor(text: string, col: number): InputBufferState {
	const before = text.slice(0, col);
	const after = text.slice(col);
	if (!before) return { text: after, cursorCol: 0 };
	const trimmed = before.replace(/\s+$/, '');
	if (!trimmed) return { text: after, cursorCol: 0 };
	// Find the last whitespace before the trailing word; keep
	// everything up to and including it.
	const lastSpace = trimmed.search(/\s\S*$/);
	const keepPrefix = lastSpace < 0 ? '' : trimmed.slice(0, lastSpace + 1);
	return { text: keepPrefix + after, cursorCol: keepPrefix.length };
}

function clamp(n: number, lo: number, hi: number): number {
	if (n < lo) return lo;
	if (n > hi) return hi;
	return n;
}
