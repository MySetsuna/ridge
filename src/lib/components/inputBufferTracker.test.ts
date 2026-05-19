import { describe, expect, it } from 'vitest';
import {
	deriveBufferEvent,
	updateInputBuffer,
	computeReplaySequence,
	EMPTY_INPUT_BUFFER,
	type InputBufferEvent,
	type InputBufferState,
	type KeySpec,
} from './inputBufferTracker';

/**
 * §1.32 (2026-05-20) — Wave C coverage.
 *
 * Locks the cursor-aware buffer state machine extracted from
 * `RidgePane.svelte::onContainerKeyDown`. Wave B's flat-string
 * approximation became `{ text, cursorCol }` so mid-line edits
 * (ArrowLeft / Home / Delete / Ctrl+W before the cursor) survive
 * a subsequent ArrowUp / history pick.
 *
 * Remaining `it.todo` markers at the bottom track Waves D/E/F.
 */

/** Cursor-at-end helper for typing-style scenarios. */
function at(text: string, cursorCol = text.length): InputBufferState {
	return { text, cursorCol };
}

/** Build a `KeySpec` with all modifiers false unless overridden. */
function key(partial: Partial<KeySpec> & { key: string }): KeySpec {
	return {
		ctrlKey: false,
		metaKey: false,
		altKey: false,
		shiftKey: false,
		...partial,
	};
}

describe('updateInputBuffer — char insertion', () => {
	it('appends a character at the end of the buffer', () => {
		expect(updateInputBuffer(at(''), { type: 'char', char: 'a' })).toEqual(at('a'));
	});

	it('appends to a non-empty buffer at end', () => {
		expect(updateInputBuffer(at('ls'), { type: 'char', char: ' ' })).toEqual(at('ls '));
	});

	it('inserts a character at the cursor mid-line and advances the cursor (Bug #3)', () => {
		// Wave B used to lose the suffix because `clear` fired on
		// ArrowLeft. Wave C keeps text+cursor through movements, so
		// inserting in the middle splices correctly.
		const state = { text: 'abef', cursorCol: 2 };
		expect(updateInputBuffer(state, { type: 'char', char: 'c' })).toEqual({
			text: 'abcef',
			cursorCol: 3,
		});
	});

	it('inserts at cursor=0 (front of buffer)', () => {
		expect(updateInputBuffer({ text: 'world', cursorCol: 0 }, { type: 'char', char: 'h' }))
			.toEqual({ text: 'hworld', cursorCol: 1 });
	});

	it('handles multi-byte / unicode characters as a single char insertion', () => {
		expect(updateInputBuffer(at('hi'), { type: 'char', char: '你' }))
			.toEqual(at('hi你'));
	});

	it('does not mutate the input state', () => {
		const before: InputBufferState = { text: 'ls', cursorCol: 2 };
		updateInputBuffer(before, { type: 'char', char: 'a' });
		expect(before).toEqual({ text: 'ls', cursorCol: 2 });
	});

	it('clamps an out-of-range cursorCol defensively (e.g. corrupted state)', () => {
		// cursorCol=99 in a 2-char buffer clamps to 2 (end).
		expect(updateInputBuffer({ text: 'ab', cursorCol: 99 }, { type: 'char', char: 'c' }))
			.toEqual({ text: 'abc', cursorCol: 3 });
		// Negative cursorCol clamps to 0.
		expect(updateInputBuffer({ text: 'ab', cursorCol: -3 }, { type: 'char', char: 'c' }))
			.toEqual({ text: 'cab', cursorCol: 1 });
	});
});

describe('updateInputBuffer — backspace', () => {
	it('removes the character BEFORE the cursor and moves cursor left', () => {
		expect(updateInputBuffer(at('echo'), { type: 'backspace' })).toEqual(at('ech'));
	});

	it('is a no-op when cursor is already at column 0', () => {
		// Pre-fix RidgePane.svelte did `buf.slice(0, -1)` which
		// silently shrank the buffer even when cursor was at 0 —
		// causing the popup query to drift from the shell line.
		expect(updateInputBuffer({ text: 'abc', cursorCol: 0 }, { type: 'backspace' }))
			.toEqual({ text: 'abc', cursorCol: 0 });
	});

	it('removes char before cursor mid-line (Bug #3)', () => {
		// State: a|bc (cursor before 'b') → backspace removes 'a'.
		expect(updateInputBuffer({ text: 'abc', cursorCol: 1 }, { type: 'backspace' }))
			.toEqual({ text: 'bc', cursorCol: 0 });
	});

	it('returns empty when single-char buffer is backspaced from end', () => {
		expect(updateInputBuffer(at('a'), { type: 'backspace' })).toEqual(at(''));
	});
});

describe('updateInputBuffer — forward delete (Bug #3)', () => {
	it('removes the character AT the cursor, cursor stays', () => {
		expect(updateInputBuffer({ text: 'abc', cursorCol: 1 }, { type: 'delete' }))
			.toEqual({ text: 'ac', cursorCol: 1 });
	});

	it('is a no-op when cursor is at end of buffer', () => {
		expect(updateInputBuffer(at('abc'), { type: 'delete' })).toEqual(at('abc'));
	});

	it('deletes the first char when cursor is at 0', () => {
		expect(updateInputBuffer({ text: 'abc', cursorCol: 0 }, { type: 'delete' }))
			.toEqual({ text: 'bc', cursorCol: 0 });
	});

	it('is a no-op on empty buffer', () => {
		expect(updateInputBuffer(EMPTY_INPUT_BUFFER, { type: 'delete' }))
			.toEqual(EMPTY_INPUT_BUFFER);
	});
});

describe('updateInputBuffer — cursor movement (Bug #3)', () => {
	it('arrowLeft decrements cursor (preserves text)', () => {
		expect(updateInputBuffer({ text: 'abc', cursorCol: 3 }, { type: 'arrowLeft' }))
			.toEqual({ text: 'abc', cursorCol: 2 });
	});

	it('arrowLeft at column 0 is a no-op', () => {
		expect(updateInputBuffer({ text: 'abc', cursorCol: 0 }, { type: 'arrowLeft' }))
			.toEqual({ text: 'abc', cursorCol: 0 });
	});

	it('arrowRight increments cursor (preserves text)', () => {
		expect(updateInputBuffer({ text: 'abc', cursorCol: 1 }, { type: 'arrowRight' }))
			.toEqual({ text: 'abc', cursorCol: 2 });
	});

	it('arrowRight at end of buffer is a no-op', () => {
		expect(updateInputBuffer(at('abc'), { type: 'arrowRight' })).toEqual(at('abc'));
	});

	it('home jumps cursor to 0', () => {
		expect(updateInputBuffer({ text: 'echo foo', cursorCol: 5 }, { type: 'home' }))
			.toEqual({ text: 'echo foo', cursorCol: 0 });
	});

	it('end jumps cursor to text.length', () => {
		expect(updateInputBuffer({ text: 'echo foo', cursorCol: 0 }, { type: 'end' }))
			.toEqual({ text: 'echo foo', cursorCol: 8 });
	});
});

describe('updateInputBuffer — Ctrl+U kill-line (Bug #4)', () => {
	it('clears the entire buffer regardless of cursor position', () => {
		expect(updateInputBuffer({ text: 'echo foo bar', cursorCol: 4 }, { type: 'killLine' }))
			.toEqual(EMPTY_INPUT_BUFFER);
	});

	it('is a no-op on empty buffer (returns the canonical empty)', () => {
		expect(updateInputBuffer(EMPTY_INPUT_BUFFER, { type: 'killLine' }))
			.toBe(EMPTY_INPUT_BUFFER);
	});
});

describe('updateInputBuffer — Ctrl+W kill-word (Bug #4)', () => {
	it('with cursor at end: drops the trailing word', () => {
		expect(updateInputBuffer(at('ls -la'), { type: 'killWord' }))
			.toEqual({ text: 'ls ', cursorCol: 3 });
	});

	it('with cursor mid-line: drops the word ending at cursor, preserves suffix (Bug #3 × Bug #4)', () => {
		// "echo foo|bar" → Ctrl+W → "echo |bar". The word "foo"
		// (before the cursor) gets killed; "bar" (after) survives.
		const state: InputBufferState = { text: 'echo foobar', cursorCol: 8 };
		expect(updateInputBuffer(state, { type: 'killWord' }))
			.toEqual({ text: 'echo bar', cursorCol: 5 });
	});

	it('with cursor at column 0: drops nothing before, preserves text and cursor', () => {
		expect(updateInputBuffer({ text: 'echo foo', cursorCol: 0 }, { type: 'killWord' }))
			.toEqual({ text: 'echo foo', cursorCol: 0 });
	});

	it('eats trailing whitespace + last word ("echo foo  " → "echo " when cursor at end)', () => {
		expect(updateInputBuffer(at('echo foo  '), { type: 'killWord' }))
			.toEqual({ text: 'echo ', cursorCol: 5 });
	});

	it('clears single-word buffer at end', () => {
		expect(updateInputBuffer(at('ls'), { type: 'killWord' })).toEqual(EMPTY_INPUT_BUFFER);
	});

	it('clears whitespace-only buffer at end', () => {
		expect(updateInputBuffer(at('   '), { type: 'killWord' })).toEqual(EMPTY_INPUT_BUFFER);
	});
});

describe('updateInputBuffer — Ctrl+K kill-to-eol (Bug #4, Bug #3 refinement)', () => {
	it('with cursor at end: no-op (nothing after to kill)', () => {
		expect(updateInputBuffer(at('echo foo'), { type: 'killToEol' }))
			.toEqual(at('echo foo'));
	});

	it('with cursor mid-line: drops suffix, cursor stays', () => {
		// Wave B used to clear the whole buffer here because we
		// couldn't model "keep prefix up to cursor". Wave C splices
		// correctly.
		const state: InputBufferState = { text: 'echo foo', cursorCol: 4 };
		expect(updateInputBuffer(state, { type: 'killToEol' }))
			.toEqual({ text: 'echo', cursorCol: 4 });
	});

	it('with cursor at column 0: clears entire buffer', () => {
		expect(updateInputBuffer({ text: 'echo foo', cursorCol: 0 }, { type: 'killToEol' }))
			.toEqual({ text: '', cursorCol: 0 });
	});

	it('is a no-op on empty buffer', () => {
		expect(updateInputBuffer(EMPTY_INPUT_BUFFER, { type: 'killToEol' }))
			.toEqual(EMPTY_INPUT_BUFFER);
	});
});

describe('updateInputBuffer — paste (Bug #6)', () => {
	it('inserts pasted text at cursor and advances cursor (Bug #3 × Bug #6)', () => {
		// "echo |" + paste "hello" → "echo hello" with cursor at 10.
		const state: InputBufferState = { text: 'echo ', cursorCol: 5 };
		expect(updateInputBuffer(state, { type: 'paste', text: 'hello' }))
			.toEqual({ text: 'echo hello', cursorCol: 10 });
	});

	it('inserts paste at mid-line position, preserves suffix', () => {
		const state: InputBufferState = { text: 'ab|ef'.replace('|', ''), cursorCol: 2 };
		expect(updateInputBuffer(state, { type: 'paste', text: 'cd' }))
			.toEqual({ text: 'abcdef', cursorCol: 4 });
	});

	it('appends pasted text when cursor is at end', () => {
		expect(updateInputBuffer(at('echo '), { type: 'paste', text: '"hello"' }))
			.toEqual({ text: 'echo "hello"', cursorCol: 12 });
	});

	it('handles multi-line pastes verbatim', () => {
		expect(updateInputBuffer(at(''), { type: 'paste', text: 'a\nb' }))
			.toEqual({ text: 'a\nb', cursorCol: 3 });
	});
});

describe('updateInputBuffer — clear', () => {
	it('returns canonical empty regardless of prior state', () => {
		expect(updateInputBuffer({ text: 'anything', cursorCol: 4 }, { type: 'clear' }))
			.toBe(EMPTY_INPUT_BUFFER);
		expect(updateInputBuffer(EMPTY_INPUT_BUFFER, { type: 'clear' }))
			.toBe(EMPTY_INPUT_BUFFER);
	});
});

describe('deriveBufferEvent — printable keys', () => {
	it('returns char event for "a"', () => {
		expect(deriveBufferEvent(key({ key: 'a' }))).toEqual({ type: 'char', char: 'a' });
	});

	it('returns char event for shifted "A"', () => {
		expect(deriveBufferEvent(key({ key: 'A', shiftKey: true })))
			.toEqual({ type: 'char', char: 'A' });
	});

	it('returns char event for space', () => {
		expect(deriveBufferEvent(key({ key: ' ' })))
			.toEqual({ type: 'char', char: ' ' });
	});

	it('returns null for Ctrl+a (we do not model select-all here)', () => {
		expect(deriveBufferEvent(key({ key: 'a', ctrlKey: true }))).toBeNull();
	});

	it('returns null for Meta+a (Cmd+a on macOS)', () => {
		expect(deriveBufferEvent(key({ key: 'a', metaKey: true }))).toBeNull();
	});
});

describe('deriveBufferEvent — readline kills (Bug #4)', () => {
	it('maps Ctrl+u to killLine', () => {
		expect(deriveBufferEvent(key({ key: 'u', ctrlKey: true })))
			.toEqual({ type: 'killLine' });
	});

	it('maps Ctrl+U (with shift, e.g. caps-on) to killLine too', () => {
		expect(deriveBufferEvent(key({ key: 'U', ctrlKey: true })))
			.toEqual({ type: 'killLine' });
	});

	it('maps Ctrl+w to killWord', () => {
		expect(deriveBufferEvent(key({ key: 'w', ctrlKey: true })))
			.toEqual({ type: 'killWord' });
	});

	it('maps Ctrl+k to killToEol', () => {
		expect(deriveBufferEvent(key({ key: 'k', ctrlKey: true })))
			.toEqual({ type: 'killToEol' });
	});

	it('does NOT match Ctrl+Shift+U (different binding)', () => {
		expect(deriveBufferEvent(key({ key: 'U', ctrlKey: true, shiftKey: true })))
			.toBeNull();
	});

	it('does NOT match Ctrl+Alt+U', () => {
		expect(deriveBufferEvent(key({ key: 'u', ctrlKey: true, altKey: true })))
			.toBeNull();
	});
});

describe('deriveBufferEvent — Backspace / Delete / cursor moves (Bug #3)', () => {
	it('maps Backspace to backspace event', () => {
		expect(deriveBufferEvent(key({ key: 'Backspace' })))
			.toEqual({ type: 'backspace' });
	});

	it('maps Delete to delete event (Wave B used to map this to clear)', () => {
		expect(deriveBufferEvent(key({ key: 'Delete' }))).toEqual({ type: 'delete' });
	});

	it('maps ArrowLeft / ArrowRight / Home / End to their own events', () => {
		expect(deriveBufferEvent(key({ key: 'ArrowLeft' }))).toEqual({ type: 'arrowLeft' });
		expect(deriveBufferEvent(key({ key: 'ArrowRight' }))).toEqual({ type: 'arrowRight' });
		expect(deriveBufferEvent(key({ key: 'Home' }))).toEqual({ type: 'home' });
		expect(deriveBufferEvent(key({ key: 'End' }))).toEqual({ type: 'end' });
	});

	it('maps Enter to clear (shell submits the line)', () => {
		expect(deriveBufferEvent(key({ key: 'Enter' }))).toEqual({ type: 'clear' });
	});

	it('does NOT map ArrowUp / ArrowDown (those open the popup; the popup-open code handles buffer state)', () => {
		expect(deriveBufferEvent(key({ key: 'ArrowUp' }))).toBeNull();
		expect(deriveBufferEvent(key({ key: 'ArrowDown' }))).toBeNull();
	});
});

describe('deriveBufferEvent — function & modifier-only keys are ignored', () => {
	it.each(['F1', 'F12', 'Escape', 'Tab', 'PageUp', 'PageDown', 'Insert', 'Meta', 'Shift', 'Control', 'Alt'])(
		'returns null for %s',
		(keyName) => {
			expect(deriveBufferEvent(key({ key: keyName }))).toBeNull();
		}
	);
});

describe('deriveBufferEvent ∘ updateInputBuffer — realistic typing scenarios', () => {
	function play(events: readonly InputBufferEvent[]): InputBufferState {
		return events.reduce<InputBufferState>(
			(s, e) => updateInputBuffer(s, e),
			EMPTY_INPUT_BUFFER,
		);
	}

	it('types "ls -la", hits Ctrl+W, types "-l" (cursor at end the whole time)', () => {
		const result = play([
			{ type: 'char', char: 'l' },
			{ type: 'char', char: 's' },
			{ type: 'char', char: ' ' },
			{ type: 'char', char: '-' },
			{ type: 'char', char: 'l' },
			{ type: 'char', char: 'a' },
			{ type: 'killWord' },
			{ type: 'char', char: '-' },
			{ type: 'char', char: 'l' },
		]);
		expect(result).toEqual({ text: 'ls -l', cursorCol: 5 });
	});

	it('types something, hits Ctrl+U, types something else', () => {
		const result = play([
			{ type: 'char', char: 'r' },
			{ type: 'char', char: 'm' },
			{ type: 'char', char: ' ' },
			{ type: 'char', char: '*' },
			{ type: 'killLine' },
			{ type: 'char', char: 'l' },
			{ type: 'char', char: 's' },
		]);
		expect(result).toEqual({ text: 'ls', cursorCol: 2 });
	});

	it('types prefix, pastes the rest', () => {
		const result = play([
			{ type: 'char', char: 'g' },
			{ type: 'char', char: 'i' },
			{ type: 'char', char: 't' },
			{ type: 'char', char: ' ' },
			{ type: 'paste', text: 'log --oneline -10' },
		]);
		expect(result).toEqual({ text: 'git log --oneline -10', cursorCol: 21 });
	});

	it('types "abc", ArrowLeft twice (cursor between "a" and "b"), inserts "X" (Bug #3 win)', () => {
		// Wave B would have dropped "ab" because ArrowLeft was a clear.
		// Wave C splices correctly.
		const result = play([
			{ type: 'char', char: 'a' },
			{ type: 'char', char: 'b' },
			{ type: 'char', char: 'c' },
			{ type: 'arrowLeft' },
			{ type: 'arrowLeft' },
			{ type: 'char', char: 'X' },
		]);
		expect(result).toEqual({ text: 'aXbc', cursorCol: 2 });
	});

	it('types, Home, types — front-insertion preserves the old text', () => {
		const result = play([
			{ type: 'char', char: 'b' },
			{ type: 'char', char: 'c' },
			{ type: 'home' },
			{ type: 'char', char: 'a' },
		]);
		expect(result).toEqual({ text: 'abc', cursorCol: 1 });
	});

	it('types, Home, Ctrl+K (clears whole line via cursor-aware kill)', () => {
		const result = play([
			{ type: 'char', char: 'a' },
			{ type: 'char', char: 'b' },
			{ type: 'home' },
			{ type: 'killToEol' },
		]);
		expect(result).toEqual({ text: '', cursorCol: 0 });
	});
});

describe('computeReplaySequence — clearing the shell line before history pick (Bug #11 / #12)', () => {
	it('returns empty string when buffer is empty (nothing to clear)', () => {
		expect(computeReplaySequence(EMPTY_INPUT_BUFFER)).toBe('');
	});

	it('returns N backspaces when cursor is at end of buffer', () => {
		// Universal case: works in any shell incl. cmd.exe.
		expect(computeReplaySequence({ text: 'echo', cursorCol: 4 })).toBe('\x08\x08\x08\x08');
	});

	it('emits Ctrl+E (\\x05) + N backspaces when cursor is mid-line (Bug #3 × Bug #11)', () => {
		// Mid-line means cursor is BEFORE the end. We move to end via
		// Ctrl+E first so the subsequent backspaces wipe the whole
		// line, not just the prefix.
		expect(computeReplaySequence({ text: 'echo foo', cursorCol: 4 }))
			.toBe('\x05\x08\x08\x08\x08\x08\x08\x08\x08');
	});

	it('emits Ctrl+E + backspaces when cursor is at column 0', () => {
		// Same shape — cursor < text.length triggers the Ctrl+E path.
		expect(computeReplaySequence({ text: 'ls', cursorCol: 0 })).toBe('\x05\x08\x08');
	});

	it('treats out-of-range cursorCol >= text.length as "at end" (no Ctrl+E needed)', () => {
		// Defensive: a future bug could leave cursorCol > text.length;
		// we still want the cheap end-of-line replay rather than an
		// unnecessary Ctrl+E that might confuse cmd.exe.
		expect(computeReplaySequence({ text: 'ab', cursorCol: 99 })).toBe('\x08\x08');
	});

	it('scales linearly with buffer length (1000-char buffer at end)', () => {
		const longText = 'x'.repeat(1000);
		expect(computeReplaySequence({ text: longText, cursorCol: 1000 }).length).toBe(1000);
	});
});

/**
 * `it.todo` markers — bugs deferred to later waves.
 */
describe('inputBufferTracker — deferred behaviours', () => {
	it.todo('syncs buffer to shell echo after Tab completion (Wave E — Bug #5)');
	it.todo('cross-checks computed replay against kernel cursor column to detect mirror drift (Wave F — design TODO)');
	it.todo('snapshots PTY-derived shell prompt suffix as a buffer source-of-truth (Wave F — design TODO)');
});
