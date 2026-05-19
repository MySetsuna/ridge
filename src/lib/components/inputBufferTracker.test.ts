import { describe, expect, it } from 'vitest';
import {
	deriveBufferEvent,
	updateInputBuffer,
	type InputBufferEvent,
	type KeySpec,
} from './inputBufferTracker';

/**
 * §1.32 (2026-05-20) — Wave B coverage.
 *
 * Locks the buffer-tracker rules extracted from
 * `RidgePane.svelte::onContainerKeyDown`. The remaining `it.todo`
 * markers at the bottom track bugs deferred to Waves C / D / E / F.
 */

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
	it('appends a single character to an empty buffer', () => {
		expect(updateInputBuffer('', { type: 'char', char: 'a' })).toBe('a');
	});

	it('appends a character to a non-empty buffer', () => {
		expect(updateInputBuffer('ls', { type: 'char', char: ' ' })).toBe('ls ');
	});

	it('handles multi-byte / unicode characters as a single char', () => {
		// `e.key` for an IME-committed CJK character is the literal
		// character (e.g. '你'). We append it as-is — the popup query
		// matches against it byte-for-byte.
		expect(updateInputBuffer('hi', { type: 'char', char: '你' })).toBe('hi你');
	});

	it('does not mutate the input buffer (callers store the return value)', () => {
		const before = 'ls';
		updateInputBuffer(before, { type: 'char', char: 'a' });
		expect(before).toBe('ls');
	});
});

describe('updateInputBuffer — backspace', () => {
	it('removes the last character', () => {
		expect(updateInputBuffer('echo', { type: 'backspace' })).toBe('ech');
	});

	it('returns empty when buffer is already empty', () => {
		// Avoid infinite '\x08' replay loops if the user holds Backspace.
		expect(updateInputBuffer('', { type: 'backspace' })).toBe('');
	});

	it('handles single-character buffer', () => {
		expect(updateInputBuffer('a', { type: 'backspace' })).toBe('');
	});
});

describe('updateInputBuffer — Ctrl+U kill-line (Bug #4)', () => {
	it('clears the entire buffer', () => {
		expect(updateInputBuffer('echo foo bar', { type: 'killLine' })).toBe('');
	});

	it('is a no-op on empty buffer', () => {
		expect(updateInputBuffer('', { type: 'killLine' })).toBe('');
	});
});

describe('updateInputBuffer — Ctrl+W kill-word (Bug #4)', () => {
	it('removes the trailing word from "ls -la" → "ls "', () => {
		expect(updateInputBuffer('ls -la', { type: 'killWord' })).toBe('ls ');
	});

	it('removes the last word from "echo foo bar" → "echo foo "', () => {
		expect(updateInputBuffer('echo foo bar', { type: 'killWord' })).toBe('echo foo ');
	});

	it('clears single-word buffer', () => {
		expect(updateInputBuffer('ls', { type: 'killWord' })).toBe('');
	});

	it('eats trailing whitespace along with the last word ("echo foo  " → "echo ")', () => {
		// Readline\'s `unix-word-rubout` removes preceding whitespace as
		// part of the kill, so users who hit Ctrl+W at the end of a
		// line get the whole word removed in one keystroke.
		expect(updateInputBuffer('echo foo  ', { type: 'killWord' })).toBe('echo ');
	});

	it('returns empty on whitespace-only buffer', () => {
		expect(updateInputBuffer('   ', { type: 'killWord' })).toBe('');
	});

	it('returns empty on empty buffer', () => {
		expect(updateInputBuffer('', { type: 'killWord' })).toBe('');
	});
});

describe('updateInputBuffer — Ctrl+K kill-to-eol (Bug #4)', () => {
	it('clears the buffer (Wave B approximation — Wave C adds column tracking)', () => {
		// Without cursor-column tracking we can't preserve the prefix
		// "up to cursor". The safe choice is to invalidate the mirror;
		// the next user keystroke rebuilds it.
		expect(updateInputBuffer('echo foo', { type: 'killToEol' })).toBe('');
	});

	it('is a no-op on empty buffer', () => {
		expect(updateInputBuffer('', { type: 'killToEol' })).toBe('');
	});
});

describe('updateInputBuffer — paste (Bug #6)', () => {
	it('appends pasted text to an empty buffer', () => {
		expect(updateInputBuffer('', { type: 'paste', text: 'git log --oneline' }))
			.toBe('git log --oneline');
	});

	it('appends pasted text to an existing buffer', () => {
		expect(updateInputBuffer('echo ', { type: 'paste', text: '"hello world"' }))
			.toBe('echo "hello world"');
	});

	it('handles multi-line pastes verbatim (shell sees them too)', () => {
		// Bracketed-paste delivers the multi-line text as one chunk
		// and the shell handles the line breaks. We mirror the same
		// text so popup filter sees what the user actually typed.
		expect(updateInputBuffer('', { type: 'paste', text: 'a\nb' })).toBe('a\nb');
	});
});

describe('updateInputBuffer — clear', () => {
	it('always returns empty regardless of prior state', () => {
		expect(updateInputBuffer('anything here', { type: 'clear' })).toBe('');
		expect(updateInputBuffer('', { type: 'clear' })).toBe('');
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
		// Ctrl+Shift+U often spawns unicode-input on Linux — must not
		// be confused with readline kill-line.
		expect(deriveBufferEvent(key({ key: 'U', ctrlKey: true, shiftKey: true })))
			.toBeNull();
	});

	it('does NOT match Ctrl+Alt+U (Alt-prefixed bindings)', () => {
		expect(deriveBufferEvent(key({ key: 'u', ctrlKey: true, altKey: true })))
			.toBeNull();
	});
});

describe('deriveBufferEvent — Backspace', () => {
	it('maps Backspace to backspace event', () => {
		expect(deriveBufferEvent(key({ key: 'Backspace' })))
			.toEqual({ type: 'backspace' });
	});
});

describe('deriveBufferEvent — cursor moves and Enter invalidate the mirror', () => {
	it.each(['Delete', 'ArrowLeft', 'ArrowRight', 'Home', 'End', 'Enter'])(
		'maps %s to clear',
		(keyName) => {
			expect(deriveBufferEvent(key({ key: keyName }))).toEqual({ type: 'clear' });
		}
	);

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
	function play(events: readonly InputBufferEvent[]): string {
		return events.reduce<string>((b, e) => updateInputBuffer(b, e), '');
	}

	it('types "ls -la", hits Ctrl+W to delete the last word, then types "-l"', () => {
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
		expect(result).toBe('ls -l');
	});

	it('types something, hits Ctrl+U to clear, types something else', () => {
		const result = play([
			{ type: 'char', char: 'r' },
			{ type: 'char', char: 'm' },
			{ type: 'char', char: ' ' },
			{ type: 'char', char: '*' },
			{ type: 'killLine' },
			{ type: 'char', char: 'l' },
			{ type: 'char', char: 's' },
		]);
		expect(result).toBe('ls');
	});

	it('types, pastes the rest of the command', () => {
		const result = play([
			{ type: 'char', char: 'g' },
			{ type: 'char', char: 'i' },
			{ type: 'char', char: 't' },
			{ type: 'char', char: ' ' },
			{ type: 'paste', text: 'log --oneline -10' },
		]);
		expect(result).toBe('git log --oneline -10');
	});

	it('types, hits ArrowLeft (mirror cleared), continues typing — Wave C will refine this', () => {
		// Locked behavior: Wave B treats cursor moves as a mirror reset.
		// Wave C will add cursor-column tracking so the prefix survives.
		const result = play([
			{ type: 'char', char: 'a' },
			{ type: 'char', char: 'b' },
			{ type: 'clear' }, // simulates ArrowLeft
			{ type: 'char', char: 'c' },
		]);
		expect(result).toBe('c');
	});
});

/**
 * `it.todo` markers — bugs deferred to later waves. Each one becomes
 * a real `it(...)` when the matching wave lands.
 */
describe('inputBufferTracker — deferred behaviours', () => {
	it.todo('tracks cursor column for ArrowLeft / ArrowRight without clearing the mirror (Wave C — Bug #3)');
	it.todo('syncs buffer to shell echo after Tab completion (Wave E — Bug #5)');
	it.todo('verifies shell line length matches buffer before sending \\x08 replay (Wave D — Bug #11/#12)');
	it.todo('snapshots PTY-derived shell prompt suffix as a buffer source-of-truth (Wave F — design TODO)');
});
