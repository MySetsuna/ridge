// src/lib/stores/fileWatcherSync.ts
//
// Glue between the filesystem watcher (backend) and the explorer + editor
// stores (frontend). Lives outside both stores so neither has to know about
// the other.
//
// Two responsibilities:
//
//  1. Compute the union of paths the user cares about (explorer column cwds
//     + open editor files that fall outside any column root) and send them
//     to the backend via `start_watching_paths`. Re-runs whenever either
//     store changes; signature-compares to avoid redundant IPC.
//
//  2. Receive `fs-changed` events, coalesce per-root over a short window,
//     and fan out to (a) the explorer (`refreshColumnsCovering`) and
//     (b) the editor (`handleExternalChange` per path).
//
// Initialised once from `+page.svelte`. Subsequent calls are no-ops.

import { get } from 'svelte/store';
import { invoke, isTauri } from '@tauri-apps/api/core';
import {
	fileExplorerStore,
	refreshColumnsCovering,
} from './fileExplorer';
import { fileEditorStore } from './fileEditor';
import { onFsChange, isRecentlyWritten, type FsChangedPayload } from './fsEvents';

interface WatchSpec {
	path: string;
	recursive: boolean;
}

let inited = false;

// ─── Watcher-spec sync ──────────────────────────────────────────────────────

let lastSentSig = '';
let syncTimer: ReturnType<typeof setTimeout> | null = null;
const SYNC_COALESCE_MS = 100;

function normalize(p: string): string {
	return p.replace(/\\/g, '/');
}

function isUnder(child: string, parent: string): boolean {
	const c = normalize(child).replace(/\/+$/, '');
	const p = normalize(parent).replace(/\/+$/, '');
	if (c === p) return true;
	return c.startsWith(p + '/');
}

function computeSpecs(): WatchSpec[] {
	const explorer = get(fileExplorerStore);
	const editor = get(fileEditorStore);

	// Step 1: every column cwd is a recursive root. Dedupe by path.
	const recursiveRoots = new Set<string>();
	for (const col of explorer.columns) {
		recursiveRoots.add(normalize(col.cwd));
	}

	// Step 2: any open file not under a recursive root needs its own
	// non-recursive watch. Diff tabs and image tabs are skipped.
	const fileRoots = new Set<string>();
	for (const f of editor.openFiles) {
		if (f.diffArgs) continue;
		const p = normalize(f.path);
		let covered = false;
		for (const root of recursiveRoots) {
			if (isUnder(p, root)) {
				covered = true;
				break;
			}
		}
		if (!covered) fileRoots.add(p);
	}

	const specs: WatchSpec[] = [];
	for (const p of recursiveRoots) specs.push({ path: p, recursive: true });
	for (const p of fileRoots) specs.push({ path: p, recursive: false });
	specs.sort((a, b) => a.path.localeCompare(b.path));
	return specs;
}

function scheduleSync(): void {
	if (syncTimer) return;
	syncTimer = setTimeout(() => {
		syncTimer = null;
		const specs = computeSpecs();
		const sig = JSON.stringify(specs);
		if (sig === lastSentSig) return;
		lastSentSig = sig;
		if (!isTauri()) return;
		void invoke('start_watching_paths', { roots: specs }).catch((e) => {
			// Don't keep the cached signature on failure — next change should retry.
			lastSentSig = '';
			console.warn('start_watching_paths failed', e);
		});
	}, SYNC_COALESCE_MS);
}

// ─── Refresh fan-out ────────────────────────────────────────────────────────

const REFRESH_DEBOUNCE_MS = 250;
const refreshTimers = new Map<string, ReturnType<typeof setTimeout>>();
const pendingPaths = new Map<string, Set<string>>();
const COALESCED_TOKEN = '__coalesced__';

function parentDirOf(p: string): string {
	const norm = normalize(p);
	const idx = norm.lastIndexOf('/');
	return idx > 0 ? norm.slice(0, idx) : norm;
}

function idleRun(cb: () => void): void {
	if (typeof requestIdleCallback === 'function') {
		requestIdleCallback(cb, { timeout: 500 });
	} else {
		setTimeout(cb, 0);
	}
}

function handleFsChange(payload: FsChangedPayload): void {
	const { root, paths, coalesced } = payload;
	let bag = pendingPaths.get(root);
	if (!bag) {
		bag = new Set();
		pendingPaths.set(root, bag);
	}
	if (coalesced) {
		bag.add(COALESCED_TOKEN);
	} else {
		// Drop paths Ridge itself just wrote — they're round-tripped saves, not
		// external changes. The file tree doesn't care about pure content edits
		// (existence/listing didn't change) and the editor explicitly silences
		// these in `handleExternalChange` too, so dropping early saves work.
		for (const p of paths) {
			if (!isRecentlyWritten(p)) bag.add(p);
		}
		if (bag.size === 0) {
			pendingPaths.delete(root);
			return;
		}
	}
	if (refreshTimers.has(root)) return;
	refreshTimers.set(
		root,
		setTimeout(() => runRefresh(root), REFRESH_DEBOUNCE_MS)
	);
}

function runRefresh(root: string): void {
	refreshTimers.delete(root);
	const bag = pendingPaths.get(root);
	pendingPaths.delete(root);
	if (!bag) return;

	idleRun(() => {
		if (bag.has(COALESCED_TOKEN)) {
			void refreshColumnsCovering(root);
			bag.delete(COALESCED_TOKEN);
		}
		// Collect parent dirs for the explorer; each unique dir → one column
		// reload (refreshColumnsCovering itself is multi-column aware).
		const dirs = new Set<string>();
		for (const p of bag) dirs.add(parentDirOf(p));
		for (const dir of dirs) {
			void refreshColumnsCovering(dir);
		}
		// Editor cares about file-level changes — feed each path through.
		for (const p of bag) {
			void fileEditorStore.handleExternalChange(p);
		}
	});
}

// ─── Init ───────────────────────────────────────────────────────────────────

/**
 * Wire up filesystem-watcher sync. Call once at app boot (e.g. from
 * `+page.svelte`'s `onMount`). Subsequent calls are no-ops.
 */
export function initFileWatcherSync(): void {
	if (inited) return;
	inited = true;

	fileExplorerStore.subscribe(() => scheduleSync());
	fileEditorStore.subscribe(() => scheduleSync());
	onFsChange(handleFsChange);
}
