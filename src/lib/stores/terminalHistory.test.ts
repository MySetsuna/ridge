import { beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

/**
 * Lock the contract for the shell-history overlay's filter / dedup
 * logic and the underlying store invariants. The popup itself moved
 * from a Svelte DOM component to a wasm canvas overlay (§1.34,
 * 2026-05-22 — see `RidgePane.svelte::openHistoryOverlay` and
 * `packages/ridge-term/src/render/renderer.rs::HistoryOverlay`), but
 * the filter / dedup helpers stayed in JS because they consume the
 * `terminalHistoryStore` and produce the `string[]` the overlay
 * paints. Unit-tested here independently of Svelte / wasm.
 */

vi.mock('@tauri-apps/api/core', () => ({
	isTauri: () => true,
	invoke: vi.fn(),
}));

const { invoke } = await import('@tauri-apps/api/core');
const mockInvoke = vi.mocked(invoke);

const mod = await import('./terminalHistory');
const { dedupKeepFirst, filterByPrefix, terminalHistoryStore, terminalHistoryLoadedStore } = mod;

beforeEach(async () => {
	mockInvoke.mockReset();
	// Reset the store to empty between tests. `terminalHistoryStore`
	// doesn't expose a setter, so we go through the public `fetch`
	// pathway with a mocked backend response. This also exercises
	// the fetch contract end-to-end as a side-benefit.
	mockInvoke.mockResolvedValueOnce([]);
	await terminalHistoryStore.fetch();
});

describe('dedupKeepFirst', () => {
	it('returns an empty array unchanged', () => {
		expect(dedupKeepFirst([])).toEqual([]);
	});

	it('returns a single-element list unchanged', () => {
		expect(dedupKeepFirst(['ls'])).toEqual(['ls']);
	});

	it('returns a list with no duplicates unchanged (stable order)', () => {
		expect(dedupKeepFirst(['ls', 'pwd', 'cd /tmp'])).toEqual(['ls', 'pwd', 'cd /tmp']);
	});

	it('keeps the FIRST occurrence of each command (newest-first store invariant)', () => {
		// On a newest-first list, "first occurrence" = "most recent use".
		// Store invariant maintained by `terminalHistoryStore.add` and the
		// backend `get_shell_history` dedup in `commands/terminal.rs`.
		expect(dedupKeepFirst(['ls', 'pwd', 'ls'])).toEqual(['ls', 'pwd']);
	});

	it('dedupes case-insensitively', () => {
		// History files sometimes contain mixed-case duplicates from
		// different shells (`ls`, `LS`, `Ls`) — treat them as one
		// command to avoid showing redundant entries.
		expect(dedupKeepFirst(['ls', 'LS', 'Ls'])).toEqual(['ls']);
	});

	it('collapses all-same entries to a single first element', () => {
		expect(dedupKeepFirst(['echo a', 'echo a', 'echo a'])).toEqual(['echo a']);
	});

	it('preserves order of non-duplicate elements interleaved with duplicates', () => {
		expect(dedupKeepFirst(['a', 'b', 'a', 'c', 'b', 'd'])).toEqual(['a', 'b', 'c', 'd']);
	});

	it('does not mutate the input list', () => {
		const input = ['ls', 'pwd', 'ls'];
		const snapshot = [...input];
		dedupKeepFirst(input);
		expect(input).toEqual(snapshot);
	});
});

describe('filterByPrefix', () => {
	const items = ['ls', 'ls -la', 'echo foo', 'echo bar', 'ECHO mixed', 'pwd'];

	it('returns a shallow copy of the input on empty query', () => {
		const out = filterByPrefix(items, '');
		expect(out).toEqual(items);
		expect(out).not.toBe(items); // shallow copy, not the same reference
	});

	it('treats whitespace-only query as "no match" (locked: prefix is literal, including spaces)', () => {
		// Implementation: `query.toLowerCase()` is "  " — truthy, so we
		// enter the prefix-match branch and look for items literally
		// starting with "  ", finding none. Locked here as the
		// deliberate semantic: shell prefixes are typed literally,
		// so whitespace is treated like any other character.
		expect(filterByPrefix(items, '  ')).toEqual([]);
	});

	it('returns matches in ascending length order with original-index tiebreaker', () => {
		// 'echo foo' (len 8) and 'echo bar' (len 8) tie on length; the
		// one earlier in the input list wins the tiebreak so the user
		// always sees the most-recent invocation first (store is
		// newest-first).
		expect(filterByPrefix(items, 'echo')).toEqual(['echo foo', 'echo bar', 'ECHO mixed']);
	});

	it('is case-insensitive — query "Echo" matches "echo foo" and "ECHO mixed"', () => {
		expect(filterByPrefix(items, 'Echo')).toEqual(['echo foo', 'echo bar', 'ECHO mixed']);
	});

	it('returns shorter matches before longer ones', () => {
		// 'ls' (len 2) before 'ls -la' (len 6).
		expect(filterByPrefix(items, 'ls')).toEqual(['ls', 'ls -la']);
	});

	it('returns empty when no item starts with the query', () => {
		expect(filterByPrefix(items, 'zzz')).toEqual([]);
	});

	it('does NOT do substring match — "bar" alone returns nothing even though "echo bar" contains it', () => {
		// Prefix-only matches the user's existing mental model
		// ("type the start, see candidates that continue"). A
		// substring fallback would surprise users who typed a
		// partial argument and got irrelevant commands.
		expect(filterByPrefix(items, 'bar')).toEqual([]);
	});

	it('handles a query equal to a full item', () => {
		expect(filterByPrefix(items, 'pwd')).toEqual(['pwd']);
	});

	it('handles a query longer than every item gracefully', () => {
		expect(filterByPrefix(items, 'pwd-but-longer')).toEqual([]);
	});

	it('does not mutate the input list', () => {
		const input = [...items];
		filterByPrefix(input, 'echo');
		expect(input).toEqual(items);
	});
});

describe('dedupKeepFirst ∘ filterByPrefix composition', () => {
	it("matches the popup's actual usage: dedup first, then filter", () => {
		// Mirrors RidgePane.svelte's `snapshotHistoryItems` derivation that
		// feeds `manager.setHistoryOverlay` (§1.34 wasm overlay).
		const store = ['ls', 'ls -la', 'ls', 'echo foo', 'echo foo', 'pwd'];
		const out = filterByPrefix(dedupKeepFirst(store), 'ls');
		expect(out).toEqual(['ls', 'ls -la']);
	});

	it('preserves newest-first ordering for the visible result', () => {
		// Store invariant: index 0 is newest. After dedup + filter the
		// shorter command sorts first (length tiebreak), and within
		// length the originally-newer entry wins.
		const store = ['echo foo', 'echo bar', 'echo foo', 'ls'];
		const out = filterByPrefix(dedupKeepFirst(store), 'echo');
		expect(out).toEqual(['echo foo', 'echo bar']);
	});
});

describe('terminalHistoryStore', () => {
	it('add() rejects whitespace-only commands', () => {
		terminalHistoryStore.add('   ');
		terminalHistoryStore.add('\t\n');
		terminalHistoryStore.add('');
		expect(get(terminalHistoryStore)).toEqual([]);
	});

	it('add() prepends the new command (newest-first invariant)', () => {
		terminalHistoryStore.add('ls');
		terminalHistoryStore.add('pwd');
		terminalHistoryStore.add('cd /tmp');
		expect(get(terminalHistoryStore)).toEqual(['cd /tmp', 'pwd', 'ls']);
	});

	it('add() removes a prior duplicate and re-inserts the command at index 0', () => {
		terminalHistoryStore.add('ls');
		terminalHistoryStore.add('pwd');
		terminalHistoryStore.add('ls'); // re-use of 'ls' lifts it to the front
		expect(get(terminalHistoryStore)).toEqual(['ls', 'pwd']);
	});

	it('add() caps history at 1000 entries', () => {
		// Push 1100 unique commands; verify only the most recent 1000
		// survive and they are in newest-first order.
		for (let i = 0; i < 1100; i++) {
			terminalHistoryStore.add(`cmd-${i}`);
		}
		const history = get(terminalHistoryStore);
		expect(history.length).toBe(1000);
		expect(history[0]).toBe('cmd-1099');
		expect(history[999]).toBe('cmd-100');
	});

	it('fetch() populates the store from the backend response', async () => {
		mockInvoke.mockResolvedValueOnce(['cmd-A', 'cmd-B', 'cmd-C']);
		await terminalHistoryStore.fetch();
		expect(get(terminalHistoryStore)).toEqual(['cmd-A', 'cmd-B', 'cmd-C']);
	});

	it('fetch() invokes the get_shell_history command with shellKind: ""', async () => {
		mockInvoke.mockResolvedValueOnce([]);
		await terminalHistoryStore.fetch();
		expect(mockInvoke).toHaveBeenCalledWith('get_shell_history', { shellKind: '' });
	});

	it('fetch() swallows backend errors without throwing', async () => {
		mockInvoke.mockRejectedValueOnce(new Error('backend offline'));
		// Should not throw; store keeps the prior value (empty after
		// the beforeEach reset).
		await expect(terminalHistoryStore.fetch()).resolves.toBeUndefined();
		expect(get(terminalHistoryStore)).toEqual([]);
	});
});

// §1.32 (2026-05-20): the 4 popup-lifecycle todos that used to live
// here were promoted to real tests in the old `historyPopupPosition`
// helper. §1.34 (2026-05-22) then moved the popup to a wasm canvas
// overlay, retiring the JS-side positioning helper entirely — its
// invariants are now enforced inside `HistoryOverlay::layout` in
// `packages/ridge-term/src/render/renderer.rs`. The dedup / filter
// rules above still apply unchanged.

describe('terminalHistoryLoadedStore', () => {
	// The `loaded` flag is consumed by RidgePane's overlay open path's
	// "no matches" early-return: openHistoryOverlay() must not open
	// the overlay while `fetch()` is still in flight, or the initial
	// empty store will collapse the open intent before its real
	// contents arrive a few ms later.
	it('flips to true after a successful fetch (beforeEach exercises this path)', () => {
		// The shared `beforeEach` calls fetch with a mocked [] response,
		// so by the time any test body runs, loaded should already be true.
		expect(get(terminalHistoryLoadedStore)).toBe(true);
	});

	it('stays true (sticky) when a later fetch fails', async () => {
		// loaded is already true from beforeEach. A subsequent error
		// must NOT flip it back — otherwise a transient backend hiccup
		// would re-enable the auto-close path and dismiss the popup
		// on the next empty-filter render.
		mockInvoke.mockRejectedValueOnce(new Error('backend offline'));
		await terminalHistoryStore.fetch();
		expect(get(terminalHistoryLoadedStore)).toBe(true);
	});

	it('stays true across multiple successful fetches', async () => {
		mockInvoke.mockResolvedValueOnce(['cmd-X']);
		await terminalHistoryStore.fetch();
		expect(get(terminalHistoryLoadedStore)).toBe(true);
		mockInvoke.mockResolvedValueOnce(['cmd-Y']);
		await terminalHistoryStore.fetch();
		expect(get(terminalHistoryLoadedStore)).toBe(true);
	});
});
