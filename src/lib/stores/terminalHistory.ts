import { writable } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';

const _store = writable<string[]>([]);
// `loaded` flips to true only after the first successful `fetch()` call.
// The popup uses this to suppress the "no matches → auto-close" branch
// during the initial Tauri IPC window: at app boot the user can hit
// ArrowUp before `get_shell_history` returns, and an empty `_store`
// would otherwise immediately dismiss the popup before its contents
// arrive a few ms later.
const _loaded = writable<boolean>(false);
export const terminalHistoryStore = {
	subscribe: _store.subscribe,
	fetch: async () => {
		try {
			const history: string[] = await invoke<string[]>('get_shell_history', { shellKind: '' });
			_store.set(history);
			_loaded.set(true);
		} catch (e) {
			console.error('Failed to fetch shell history', e);
		}
	},
	add: (command: string) => {
		if (!command.trim()) return;
		_store.update(history => {
			const newHistory = [command, ...history.filter(h => h !== command)];
			return newHistory.slice(0, 1000);
		});
	}
};
export const terminalHistoryLoadedStore = { subscribe: _loaded.subscribe };

// §1.31 (2026-05-19): pure filter / dedup helpers, originally extracted
// from the inline IIFE in the old DOM popup (`TerminalHistoryPopup.svelte`,
// retired by §1.34 wasm overlay). Now consumed by `RidgePane.svelte`'s
// `snapshotHistoryItems` which feeds `manager.setHistoryOverlay`.
// Tested in Vitest — previously accumulated a handful of subtle bugs
// (case sensitivity, multi-line entries, sort tiebreakers).

/**
 * Case-insensitive dedup that keeps the FIRST occurrence in `items`.
 *
 * Why "keep first" is the right semantic here: the store invariant
 * maintained by `terminalHistoryStore.add` is **newest-first** (it
 * prepends and removes prior duplicates on every add). On top of a
 * newest-first list, "keep first occurrence" means "keep the most
 * recent invocation of each command" — which is what the popup is
 * supposed to display. The matching backend command
 * (`get_shell_history` in `src-tauri/src/commands/terminal.rs`)
 * reverses the on-disk history file before deduping, so its output
 * also lands in newest-first order. Same invariant on both ends.
 *
 * Stable: the relative order of non-duplicate items is preserved.
 */
export function dedupKeepFirst(items: readonly string[]): string[] {
	const seen = new Set<string>();
	const out: string[] = [];
	for (const item of items) {
		const key = item.toLowerCase();
		if (seen.has(key)) continue;
		seen.add(key);
		out.push(item);
	}
	return out;
}

/**
 * Case-insensitive prefix match against `query`. Empty / whitespace
 * query returns a shallow copy of `items` unchanged.
 *
 * Order: **newest-first**, preserving the input order (which the store +
 * `dedupKeepFirst` maintain as most-recent-first). The matches are pushed
 * in input order so the most recently run commands stay at the top — like
 * Warp's history. (Previously this sorted by command length, surfacing the
 * shortest match first regardless of recency, which buried the command the
 * user most likely wants.)
 *
 * The function is intentionally **prefix-only**, not substring or
 * fuzzy — matches the user's current shell-history mental model
 * ("type the start of the command, see candidates that continue it").
 */
export function filterByPrefix(items: readonly string[], query: string): string[] {
	const q = query.toLowerCase();
	if (!q) return [...items];
	const out: string[] = [];
	for (const item of items) {
		if (item.toLowerCase().startsWith(q)) out.push(item);
	}
	return out;
}
