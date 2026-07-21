import { describe, expect, it } from 'vitest';
import {
	reconstructInputSnapshot,
	type CellLike,
} from './shellInputSnapshot';

/**
 * Wave F (§1.32) — lock the PTY-prompt suffix reconstruction.
 *
 * The kernel's `cellsAt(row, col, len)` returns an array of cells;
 * we hand-craft those arrays here to test every edge case the
 * shell can throw at us:
 *
 *   - Pre-cursor / post-cursor splits at the right code unit.
 *   - Trailing blank cells from end-of-row fill are trimmed.
 *   - Trailing whitespace the user genuinely typed survives.
 *   - CJK / wide chars contribute ONE code unit but occupy TWO
 *     cells (width=2 leader + width=0 continuation).
 *   - Empty buffers / empty input start return empty snapshots.
 */

/** Helper: build width-1 ASCII cells from a string. */
function ascii(s: string): CellLike[] {
	return Array.from(s, (ch) => ({ ch, width: 1 }));
}

/** Helper: build width-1 blank "fill" cells (kernel's end-of-row pad). */
function blanks(n: number): CellLike[] {
	return Array.from({ length: n }, () => ({ ch: ' ', width: 1 }));
}

/** Helper: build a wide-char cell pair (leader + continuation). */
function wide(ch: string): CellLike[] {
	return [
		{ ch, width: 2 },
		{ ch: '', width: 0 },
	];
}

describe('reconstructInputSnapshot — basic cases', () => {
	it('returns empty snapshot when both ranges are empty', () => {
		expect(reconstructInputSnapshot([], [])).toEqual({ text: '', cursorCol: 0 });
	});

	it('returns empty text when only the post range has blank fill (cursor at empty prompt)', () => {
		expect(reconstructInputSnapshot([], blanks(20))).toEqual({ text: '', cursorCol: 0 });
	});

	it('reconstructs pre-cursor text (cursor at end of typed input)', () => {
		expect(reconstructInputSnapshot(ascii('ls'), blanks(78)))
			.toEqual({ text: 'ls', cursorCol: 2 });
	});

	it('reconstructs cursor mid-line text', () => {
		const pre = ascii('echo ');
		const post = [...ascii('foo'), ...blanks(50)];
		expect(reconstructInputSnapshot(pre, post))
			.toEqual({ text: 'echo foo', cursorCol: 5 });
	});

	it('reconstructs cursor at column 0 (user pressed Home)', () => {
		const pre: CellLike[] = [];
		const post = [...ascii('echo'), ...blanks(40)];
		expect(reconstructInputSnapshot(pre, post))
			.toEqual({ text: 'echo', cursorCol: 0 });
	});
});

describe('reconstructInputSnapshot — trailing whitespace handling', () => {
	it('preserves trailing whitespace in PRE-cursor segment (user typed "ls   |")', () => {
		// The trailing spaces are typed input, not row fill — they live
		// before the cursor and must survive.
		expect(reconstructInputSnapshot(ascii('ls   '), blanks(40)))
			.toEqual({ text: 'ls   ', cursorCol: 5 });
	});

	it('strips trailing whitespace from POST-cursor segment (row fill)', () => {
		expect(reconstructInputSnapshot(ascii('a'), [...ascii('b'), ...blanks(40)]))
			.toEqual({ text: 'ab', cursorCol: 1 });
	});

	it('does NOT strip whitespace BETWEEN typed text in post-cursor', () => {
		expect(reconstructInputSnapshot(ascii('a'), [...ascii('b c'), ...blanks(40)]))
			.toEqual({ text: 'ab c', cursorCol: 1 });
	});

	it('preserves blank pre-cursor cells while trimming post-cursor blanks', () => {
		// Edge: empty prompt with blanks around the cursor.
		expect(reconstructInputSnapshot(blanks(2), blanks(40)))
			.toEqual({ text: '  ', cursorCol: 2 });
	});
});

describe('reconstructInputSnapshot — wide chars (CJK)', () => {
	it('treats a wide-char leader as ONE code unit (not two)', () => {
		expect(reconstructInputSnapshot(wide('你'), blanks(40)))
			.toEqual({ text: '你', cursorCol: 1 });
	});

	it('reconstructs multi-CJK input correctly', () => {
		const pre = [...wide('你'), ...wide('好')];
		expect(reconstructInputSnapshot(pre, blanks(40)))
			.toEqual({ text: '你好', cursorCol: 2 });
	});

	it('handles cursor between wide chars', () => {
		const pre = wide('你');
		const post = [...wide('好'), ...blanks(40)];
		expect(reconstructInputSnapshot(pre, post))
			.toEqual({ text: '你好', cursorCol: 1 });
	});

	it('mixes ASCII and wide chars', () => {
		const pre = [...ascii('ls '), ...wide('你'), ...wide('好')];
		expect(reconstructInputSnapshot(pre, blanks(40)))
			.toEqual({ text: 'ls 你好', cursorCol: 5 });
	});

	it('skips width-0 continuation cells defensively (even at the boundary)', () => {
		const orphanCont: CellLike = { ch: '', width: 0 };
		const pre = [orphanCont, ...ascii('a')];
		expect(reconstructInputSnapshot(pre, blanks(40)))
			.toEqual({ text: 'a', cursorCol: 1 });
	});
});

describe('reconstructInputSnapshot — realistic shell scenarios', () => {
	it('captures the post-Tab-completion line ("ec" + Tab → "echo ")', () => {
		expect(reconstructInputSnapshot(ascii('echo '), blanks(40)))
			.toEqual({ text: 'echo ', cursorCol: 5 });
	});

	it('captures cursor mid-line after ArrowLeft past a Tab completion (with known trailing-space limitation)', () => {
		// LIMITATION: a "trailing space" in the post-cursor segment is
		// indistinguishable from the row's end-of-line blank fill
		// (both are width-1 ' ' cells in the kernel grid). So the
		// snapshot returns "echo" not "echo " here, even though the
		// shell actually has "echo " on screen. The replay loses 1
		// backspace's worth of accuracy in this edge case.
		//
		// In practice this matters only for the (rare) scenario where
		// the user typed trailing whitespace, then ArrowLeft past it,
		// then opened the history popup. The keystroke mirror (Wave B
		// fallback) handles this case fine because it never reads the
		// grid; for the snapshot path we accept the lossiness as the
		// cost of "no shell-side surprise can ever drift us".
		expect(reconstructInputSnapshot(ascii('ec'), [...ascii('ho '), ...blanks(40)]))
			.toEqual({ text: 'echo', cursorCol: 2 });
	});

	it('captures a long command at the right boundary of the row', () => {
		const longCmd = 'git log --oneline --decorate=full --color=always | head -20';
		expect(reconstructInputSnapshot(ascii(longCmd), blanks(0)))
			.toEqual({ text: longCmd, cursorCol: longCmd.length });
	});

	it('handles a $VAR expansion ("ls $HOME" → "ls /home/user")', () => {
		// Bash-like expansion: the shell shows "ls /home/user" after
		// the expansion. Our snapshot reads what's actually rendered.
		expect(reconstructInputSnapshot(ascii('ls /home/user'), blanks(40)))
			.toEqual({ text: 'ls /home/user', cursorCol: 13 });
	});
});
