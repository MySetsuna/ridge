import { writable, get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';

const _store = writable<string[]>([]);
export const terminalHistoryStore = {
    subscribe: _store.subscribe,
    fetch: async () => {
        try {
            const history: string[] = await invoke<string[]>('get_shell_history', { shellKind: '' });
            _store.set(history);
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

// §1.31 (2026-05-19): pure helpers extracted from the inline IIFE that
// used to live in `TerminalHistoryPopup.svelte` (lines 13-29 of the old
// version). Extracted so the popup's filter/dedup behaviour can be
// truth-tested in Vitest — the previous inline form shipped untested
// and accumulated a handful of subtle bugs (case sensitivity, multi-
// line entries, sort tiebreakers).

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
 * Sort order on matches: ascending by command length, then by the
 * item's index in `items` (stable tiebreaker). This mirrors the
 * popup's original behaviour so a drop-in swap is invisible to the
 * user.
 *
 * The function is intentionally **prefix-only**, not substring or
 * fuzzy — matches the user's current shell-history mental model
 * ("type the start of the command, see candidates that continue it").
 */
export function filterByPrefix(items: readonly string[], query: string): string[] {
    const q = query.toLowerCase();
    if (!q) return [...items];
    const matches: { cmd: string; originalIndex: number }[] = [];
    for (let i = 0; i < items.length; i++) {
        if (items[i].toLowerCase().startsWith(q)) {
            matches.push({ cmd: items[i], originalIndex: i });
        }
    }
    matches.sort((a, b) => a.cmd.length - b.cmd.length || a.originalIndex - b.originalIndex);
    return matches.map(m => m.cmd);
}
