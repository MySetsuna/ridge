import { describe, it } from 'vitest';

/**
 * §1.31 (2026-05-20) — deferred test skeleton.
 *
 * This file intentionally contains only `it.todo` markers: it
 * documents the "buffer-drifts-from-shell-line" cluster of bugs
 * scoped OUT of the current TUI-gate / history-popup fix.
 *
 * The current implementation in `RidgePane.svelte::onContainerKeyDown`
 * tracks the user's typed-since-last-popup characters in a local
 * `currentInputBuffer` string and, on history pick, sends
 * `'\x08'.repeat(currentInputBuffer.length)` to the shell before
 * writing the selected command. That assumes the shell's input line
 * is exactly `currentInputBuffer` long — which is wrong whenever the
 * user has done anything besides plain printable-key typing.
 *
 * Each `it.todo` below describes a concrete failure mode the
 * implementation should eventually fix. When a fix lands, promote
 * the matching `it.todo(...)` to a real `it(...)` test. The plan
 * file at `~/.claude/plans/tui-shell-tui-shell-bug-lively-walrus.md`
 * tracks the bug numbers referenced here.
 *
 * No production code is imported — the buffer logic is still inline
 * in `RidgePane.svelte`. A future refactor should extract it into
 * a pure `inputBufferTracker.ts` module so each todo can become a
 * normal unit test.
 */

describe('inputBufferTracker — buffer-vs-shell-line sync (deferred)', () => {
	it.todo('clears buffer on Ctrl+U (readline kill-line) — Bug #4');
	it.todo('clears buffer on Ctrl+W (kill-word) — Bug #4');
	it.todo('clears buffer on Ctrl+K (kill-to-end-of-line) — Bug #4');
	it.todo('syncs buffer to shell echo after Tab completion — Bug #5');
	it.todo('syncs buffer when text is pasted via IME helper — Bug #6');
	it.todo('tracks cursor column when ArrowLeft / ArrowRight moves mid-line — Bug #3');
	it.todo('verifies shell line length matches buffer before sending \\x08 replay — Bug #11 / #12');
	it.todo('snapshots PTY-derived shell prompt suffix as a buffer source-of-truth (replaces local mirror) — design TODO');
});
