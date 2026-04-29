// src/lib/stores/fsEvents.ts
//
// Filesystem-change event bus.
//
// Subscribes once (lazily) to the Tauri `fs-changed` event emitted by
// `src-tauri/src/commands/fs_watch.rs` and fans out to any number of consumers
// (file tree, editor) via `onFsChange()`.
//
// Also provides a "recently-written" suppression window so files saved by Ridge
// itself (`write_file`) don't trigger an external-modification reload prompt
// in the editor (the watcher will see our own write and round-trip back).

import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { isTauri } from '@tauri-apps/api/core';

export interface FsChangedPayload {
	root: string;
	paths: string[];
	/** Backend over-emit guard — too many paths in one debounce window. */
	coalesced: boolean;
}

export type FsChangeHandler = (payload: FsChangedPayload) => void;

const handlers = new Set<FsChangeHandler>();

let unlisten: UnlistenFn | null = null;
let initPromise: Promise<void> | null = null;

async function ensureListener(): Promise<void> {
	if (unlisten || !isTauri()) return;
	if (initPromise) return initPromise;
	initPromise = (async () => {
		try {
			unlisten = await listen<FsChangedPayload>('fs-changed', (evt) => {
				const payload = evt.payload;
				for (const h of handlers) {
					try {
						h(payload);
					} catch (e) {
						console.warn('fs-changed handler threw', e);
					}
				}
			});
		} catch (e) {
			console.warn('failed to subscribe fs-changed', e);
			initPromise = null;
		}
	})();
	return initPromise;
}

/**
 * Register a handler for fs-changed events. Returns an unsubscribe.
 * The Tauri listener is set up lazily on first registration and torn down
 * when the last handler unsubscribes.
 */
export function onFsChange(handler: FsChangeHandler): () => void {
	handlers.add(handler);
	void ensureListener();
	return () => {
		handlers.delete(handler);
		if (handlers.size === 0 && unlisten) {
			const off = unlisten;
			unlisten = null;
			initPromise = null;
			off();
		}
	};
}

// ─── Recently-written suppression ───────────────────────────────────────────
// Window covers: write syscall return → fs flush → notify event arrival →
// debouncer flush (250ms). 800ms is the worst-case sum we expect.

const RECENTLY_WRITTEN_MS = 800;
const recentlyWritten = new Map<string, ReturnType<typeof setTimeout>>();

/** Normalize for compare (windows backslash → forward slash). */
function normalizeKey(path: string): string {
	return path.replace(/\\/g, '/');
}

/**
 * Mark a file path as "Ridge just wrote this". For the next ~800ms, fs-changed
 * events for this path should be ignored by editor-reload logic so we don't
 * round-trip the user's own save back through a "file changed externally"
 * prompt.
 */
export function markRecentlyWritten(path: string): void {
	const key = normalizeKey(path);
	const prev = recentlyWritten.get(key);
	if (prev) clearTimeout(prev);
	const handle = setTimeout(() => {
		recentlyWritten.delete(key);
	}, RECENTLY_WRITTEN_MS);
	recentlyWritten.set(key, handle);
}

/** Did Ridge itself write this path within the suppression window? */
export function isRecentlyWritten(path: string): boolean {
	return recentlyWritten.has(normalizeKey(path));
}
