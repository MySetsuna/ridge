/**
 * §1.32 (2026-05-20) — pure state machine for the shell-input mirror
 * used as a fast-path for terminal input.
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
 *  (= text.length means "cursor is at end of line"). `dirty` is set
 *  when we know the shell line diverged from our mirror — currently
 *  Tab completion is the only trigger (the PTY echoes completed
 *  text that our keystroke-driven mirror can't see). `dirty` is
 *  optional for ergonomic state-literal construction; `undefined`
 *  is treated as `false`. Treat this struct as immutable — every
 *  event returns a fresh value. */
export interface InputBufferState {
	readonly text: string;
	readonly cursorCol: number;
	readonly dirty?: boolean;
}

/** Initial / reset state. Same reference is reused for `clear` /
 *  `killLine`; identity comparison is safe for "is this empty?". */
export const EMPTY_INPUT_BUFFER: InputBufferState = Object.freeze({
	text: '',
	cursorCol: 0,
});

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
	| { type: 'tab' }
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
	// §1.32 Wave E: Tab triggers shell-side completion that echoes
	// text we never see as keystrokes. Mark the mirror dirty so the
	// `\x08` replay uses the kill-line shortcut (`\x05\x15`) instead
	// of relying on a stale length count.
	if (spec.key === 'Tab' && !spec.ctrlKey && !spec.metaKey && !spec.altKey && !spec.shiftKey) {
		return { type: 'tab' };
	}
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
	const dirty = state.dirty === true;
	switch (ev.type) {
		case 'char': {
			const newText = text.slice(0, col) + ev.char + text.slice(col);
			return mkState(newText, col + ev.char.length, dirty);
		}
		case 'backspace': {
			if (col === 0) return mkState(text, col, dirty);
			return mkState(text.slice(0, col - 1) + text.slice(col), col - 1, dirty);
		}
		case 'delete': {
			if (col >= text.length) return mkState(text, col, dirty);
			return mkState(text.slice(0, col) + text.slice(col + 1), col, dirty);
		}
		case 'arrowLeft':
			return mkState(text, col === 0 ? 0 : col - 1, dirty);
		case 'arrowRight':
			return mkState(text, col >= text.length ? text.length : col + 1, dirty);
		case 'home':
			return mkState(text, 0, dirty);
		case 'end':
			return mkState(text, text.length, dirty);
		case 'killLine':
			// Ctrl+U fully clears the shell line on its end too — so the
			// dirty bit can come off.
			return EMPTY_INPUT_BUFFER;
		case 'killWord': {
			const next = killWordAtCursor(text, col);
			return mkState(next.text, next.cursorCol, dirty);
		}
		case 'killToEol':
			return mkState(text.slice(0, col), col, dirty);
		case 'paste': {
			const newText = text.slice(0, col) + ev.text + text.slice(col);
			return mkState(newText, col + ev.text.length, dirty);
		}
		case 'tab':
			// Tab makes the shell echo completed text our mirror can't
			// see. Keep `text` and `cursorCol` (they're our best guess
			// for the user's typed prefix and what filter to apply in
			// the history popup) but mark the mirror dirty so the
			// `\x08` replay switches to `\x05\x15` (kill-line) which
			// works regardless of how much the shell completion added.
			return mkState(text, col, true);
		case 'clear':
			return EMPTY_INPUT_BUFFER;
	}
}

/** Build an `InputBufferState`. `dirty: true` is materialised only
 *  when actually dirty so test fixtures that omit `dirty` from their
 *  expected literals still match via Vitest's `toEqual`. */
function mkState(text: string, cursorCol: number, dirty: boolean): InputBufferState {
	return dirty ? { text, cursorCol, dirty: true } : { text, cursorCol };
}

/**
 * Drop the trailing word *before the cursor* (matching GNU readline's
 * `unix-word-rubout` Ctrl+W), preserving whatever text was already
 * past the cursor. Cursor lands where the kill left off. The caller
 * stitches the `dirty` bit back on — this helper only computes
 * `text` and `cursorCol`.
 */
function killWordAtCursor(text: string, col: number): { text: string; cursorCol: number } {
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

/**
 * §1.32 (2026-05-20) Wave D — compute the byte sequence to send to
 * the PTY before writing the picked history command, so the shell's
 * current input line is wiped out cleanly even when the cursor was
 * mid-line.
 *
 *   - Empty buffer → nothing to clear, return "".
 *   - Cursor at end → `\x08` × text.length (universal backspace,
 *     works in cmd.exe too).
 *   - Cursor mid-line → `\x05` (Ctrl+E, move to end of line) +
 *     `\x08` × text.length. Readline shells (zsh, bash, PSReadLine)
 *     all support `\x05`; cmd.exe does not, but cmd.exe users rarely
 *     navigate mid-line anyway, and the fallback degradation is just
 *     "some trailing garbage on the line" — strictly less wrong than
 *     the pre-Wave-D naive backspace-only which left the suffix
 *     intact regardless of cursor position.
 *
 * Wave F (PTY-prompt suffix snapshot) cross-checks against the
 * kernel's actual cursor column before the replay (see
 * `manager.readShellInputSnapshot`) so mirror drift is detected and
 * we bail to the keystroke mirror instead of sending too few / too
 * many backspaces.
 */
export function computeReplaySequence(state: InputBufferState): string {
	// Wave E: when the mirror is dirty (e.g. Tab completion echoed
	// text we can't see) the byte-count length is unreliable. Fall
	// back to `\x05\x15` — Ctrl+E (move to end) + Ctrl+U (kill from
	// cursor to start). Readline shells wipe the line cleanly in two
	// bytes regardless of length. cmd.exe doesn't honour these but
	// users hitting Tab at a cmd.exe prompt is rare.
	if (state.dirty) return '\x05\x15';
	if (state.text.length === 0) return '';
	const cursorAtEnd = state.cursorCol >= state.text.length;
	const backspaces = '\x08'.repeat(state.text.length);
	return cursorAtEnd ? backspaces : '\x05' + backspaces;
}

