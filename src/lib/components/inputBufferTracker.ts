/**
 * §1.32 (2026-05-20) — pure state machine for the shell-input mirror
 * that the history-popup uses as its `query` and as the `\x08` replay
 * count when the user picks a history entry.
 *
 * Why a separate module: the same logic used to live inline in
 * `RidgePane.svelte::onContainerKeyDown` and was untested. Every
 * keystroke path that mutates `currentInputBuffer` is a chance to
 * drift from the shell's actual line — the agent triage found 8
 * concrete bugs (Ctrl+U/W/K kills, Tab completion, paste sync,
 * cursor moves, backspace replay validation). Extracting the rules
 * here lets us:
 *
 *   1. Test each event type without DOM / Svelte plumbing.
 *   2. Fix one event at a time without touching the component.
 *   3. Mark deferred events with `it.todo` against the same module
 *      so future work has a concrete promotion target.
 *
 * Current coverage (Wave B):
 *   - char insertion
 *   - backspace
 *   - clear (cursor moves / Enter / Delete invalidate the mirror)
 *   - paste (text-literal append)
 *   - Ctrl+U kill-line     → buffer cleared
 *   - Ctrl+W kill-word     → drops trailing word + trailing whitespace
 *   - Ctrl+K kill-to-eol   → buffer cleared (Wave C will refine with
 *     cursor-column tracking; the safe-approximation for now is "this
 *     mutates the shell line in a way our flat string can't model →
 *     invalidate and let the next keystroke rebuild")
 *
 * Deferred (still `it.todo`):
 *   - cursor-column tracking for ArrowLeft / ArrowRight mid-line (Wave C)
 *   - Tab completion echo sync (Wave E)
 *   - shell-line length validation before `\x08` replay (Wave D)
 */

/** Events the buffer state machine understands. */
export type InputBufferEvent =
	| { type: 'char'; char: string }
	| { type: 'backspace' }
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
 * key doesn't affect the mirror (modifier-only press, function keys,
 * etc.). Pure function — no DOM access.
 *
 * Readline kill bindings (Ctrl+U / Ctrl+W / Ctrl+K) are recognised
 * across platforms — even macOS shells bind these to Ctrl, not Cmd.
 *
 * Cursor moves (ArrowLeft / ArrowRight / Home / End) and Delete /
 * Enter all return `clear` until Wave C lands proper column tracking:
 * any operation that mutates the shell line at a position we don't
 * model is treated as "invalidate the mirror" — safer than letting
 * `\x08` replay send the wrong number of backspaces.
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
	// Cursor moves / Delete / Enter all break our flat-string model
	// (we can't tell which column was deleted from / which prefix
	// survives). Invalidate the mirror.
	if (
		spec.key === 'Delete' ||
		spec.key === 'ArrowLeft' ||
		spec.key === 'ArrowRight' ||
		spec.key === 'Home' ||
		spec.key === 'End' ||
		spec.key === 'Enter'
	) {
		return { type: 'clear' };
	}
	return null;
}

/**
 * Apply an event to the current buffer and return the new buffer.
 * Pure: input is not mutated. Callers store the return value.
 */
export function updateInputBuffer(buf: string, ev: InputBufferEvent): string {
	switch (ev.type) {
		case 'char':
			return buf + ev.char;
		case 'backspace':
			return buf.slice(0, -1);
		case 'killLine':
			return '';
		case 'killWord':
			return dropLastWord(buf);
		case 'killToEol':
			// Without column tracking we can't preserve the prefix.
			// Wave C will refine this to slice up to the cursor.
			return '';
		case 'paste':
			return buf + ev.text;
		case 'clear':
			return '';
	}
}

/**
 * Drop the trailing word and any preceding whitespace, matching
 * GNU readline's Ctrl+W (`unix-word-rubout`):
 *   - "ls"           → ""
 *   - "ls -la"       → "ls "
 *   - "echo foo bar" → "echo foo "
 *   - "echo foo  "   → "echo " (trailing whitespace included in the kill)
 *   - ""             → ""
 */
function dropLastWord(buf: string): string {
	if (!buf) return '';
	const trimmedRight = buf.replace(/\s+$/, '');
	if (!trimmedRight) return '';
	// Find the last whitespace before the trailing word; keep
	// everything up to and including it.
	const lastSpace = trimmedRight.search(/\s\S*$/);
	return lastSpace < 0 ? '' : trimmedRight.slice(0, lastSpace + 1);
}
