// src/lib/stores/fileExplorer.ts
import { writable, get, derived } from 'svelte/store';
import { invoke, isTauri } from '@tauri-apps/api/core';
import type { FileNode } from './project';
import { paneCwdStore } from './paneTree';
import { reportDevIssue } from '$lib/devIssue';

export interface ExplorerColumn {
	id: string; // "${workspaceId}:${cwd}" — CWD-based key so panes sharing CWD share a column
	workspaceId: string;
	paneIds: string[]; // all panes currently at this CWD
	/** Display titles for each pane in paneIds, keyed by paneId. */
	paneTitles: Record<string, string>;
	cwd: string;
	rootPath: string;
	expandedPaths: Set<string>;
	/**
	 * Primary selected path — the "cursor" row. Keyboard focus lives here,
	 * ArrowUp/Down moves from here, Delete/F2 target this path.
	 * For backward compatibility this stays a single string; multi-select
	 * lives in `selectedPaths` as a superset.
	 */
	selectedPath: string | null;
	/**
	 * Full selection set. Always contains `selectedPath` when non-null.
	 * Single click replaces; Ctrl-click toggles; Shift-click ranges.
	 * Not persisted to localStorage (matches VS Code: multi-select resets
	 * on reload, only the primary selection survives).
	 */
	selectedPaths: Set<string>;
	/**
	 * Shift-range anchor. Set to `selectedPath` on every plain or Ctrl click;
	 * a subsequent Shift-click extends from this anchor to the target.
	 */
	anchorPath: string | null;
	tree: FileNode | null;
	loading: boolean;
	/**
	 * True iff the cached tree is suspected stale (e.g. a new pane joined this
	 * CWD). Explorer triggers a background loadTree and drops the flag.
	 */
	needsRefresh?: boolean;
}

/** Workspace descriptor used for multi-workspace sync. */
export interface WorkspaceDescriptor {
	id: string;
	name?: string;
	index: number;
}

/** Grouped view: one entry per workspace, with cwd-groups inside. */
export interface ExplorerWorkspaceGroup {
	workspaceId: string;
	workspaceName: string;
	columns: ExplorerColumn[];
	collapsed: boolean;
}

export interface FileExplorerState {
	columns: ExplorerColumn[];
	columnOrder: string[]; // ordered column ids for drag reorder
	activeColumnId: string | null;
}

const initialState: FileExplorerState = {
	columns: [],
	columnOrder: [],
	activeColumnId: null,
};

function cwdColumnId(workspaceId: string, cwd: string): string {
	return `${workspaceId}:${cwd}`;
}

// ─── Per-column persistence (expandedPaths + selectedPath) ───────────────────
// Stored under one localStorage key per column so reading a single workspace
// doesn't deserialise every other workspace's state. Hard-capped at 500 paths
// per column to keep the payload small on giant mono-repos (LRU-like — we
// drop the oldest when serialising if over the cap).
const LS_PREFIX = 'ridge-explorer-column:';
const MAX_EXPANDED = 500;

interface PersistedColumnState {
	expanded: string[];
	selected: string | null;
}

function lsKeyFor(columnId: string): string {
	return `${LS_PREFIX}${columnId}`;
}

function loadPersistedColumn(columnId: string): PersistedColumnState | null {
	if (typeof localStorage === 'undefined') return null;
	try {
		const raw = localStorage.getItem(lsKeyFor(columnId));
		if (!raw) return null;
		const parsed = JSON.parse(raw) as Partial<PersistedColumnState>;
		const expanded = Array.isArray(parsed.expanded) ? parsed.expanded.filter((p) => typeof p === 'string') : [];
		const selected = typeof parsed.selected === 'string' ? parsed.selected : null;
		return { expanded, selected };
	} catch {
		return null;
	}
}

function savePersistedColumn(col: ExplorerColumn): void {
	if (typeof localStorage === 'undefined') return;
	try {
		const expanded = Array.from(col.expandedPaths).slice(-MAX_EXPANDED);
		const payload: PersistedColumnState = { expanded, selected: col.selectedPath };
		localStorage.setItem(lsKeyFor(col.id), JSON.stringify(payload));
	} catch {
		/* ignore quota / privacy errors — persistence is best-effort */
	}
}

function createFileExplorerStore() {
	const { subscribe, set, update } = writable<FileExplorerState>(initialState);

	return {
		subscribe,

		/**
		 * Sync columns with pane CWDs for a workspace.
		 * Terminals sharing the same CWD share a single column (file tree).
		 * When a pane's CWD changes, it moves to the matching column (or creates one).
		 * Columns with no remaining panes are removed.
		 * Columns belonging to OTHER workspaces are left completely untouched.
		 *
		 * @param paneCwds   Map of paneId → cwd for this workspace.
		 * @param paneTitles Optional map of paneId → display title (kept across syncs).
		 */
		syncWithPaneCwds(
			workspaceId: string,
			paneCwds: Record<string, string>,
			paneTitles: Record<string, string> = {}
		): void {
			update((state) => {
				// Build a map: cwd → list of paneIds
				const cwdToPanes = new Map<string, string[]>();
				for (const [paneId, cwd] of Object.entries(paneCwds)) {
					if (!cwdToPanes.has(cwd)) cwdToPanes.set(cwd, []);
					cwdToPanes.get(cwd)!.push(paneId);
				}

				const existingById = new Map(
					state.columns.filter((c) => c.workspaceId === workspaceId).map((c) => [c.id, c])
				);

				const newColumns: ExplorerColumn[] = [];

				// Keep columns from other workspaces unchanged
				for (const col of state.columns) {
					if (col.workspaceId !== workspaceId) newColumns.push(col);
				}

				for (const [cwd, paneIds] of cwdToPanes) {
					const colId = cwdColumnId(workspaceId, cwd);
					const existing = existingById.get(colId);
					// Merge incoming titles with any previously stored titles
					const mergedTitles: Record<string, string> = {
						...(existing?.paneTitles ?? {}),
						...paneTitles,
					};
					if (existing) {
						// 检测是否有新 pane 加入到这个既有列（说明可能是重进/切回到老目录）。
						// 重进老目录时，缓存的 tree 多半已过时 —— 但直接 `tree = null` 会导致
						// 视图先空屏再填充，形成"打开文件夹时的闪烁"。改为：保留旧树原地显示，
						// 在 Explorer 层异步调用 loadTree 做后台刷新，数据就绪后再原子替换。
						const prevSet = new Set(existing.paneIds);
						const hasNewJoiner = paneIds.some((id) => !prevSet.has(id));
						// Only request a refresh when the tree hasn't loaded yet.
						// If a cached tree already exists, a new pane sharing this cwd
						// reuses it without re-scanning (the user can always hit the
						// refresh button to force an update).
						newColumns.push({
							...existing,
							paneIds,
							paneTitles: mergedTitles,
							tree: existing.tree,
							needsRefresh: existing.needsRefresh || (hasNewJoiner && existing.tree === null),
						});
					} else {
						// New CWD: create column, tree: null triggers load.
						// Rehydrate expanded + selected state from localStorage so
						// re-opening the app returns to the last-seen shape.
						const persisted = loadPersistedColumn(colId);
						const primary = persisted?.selected ?? null;
						newColumns.push({
							id: colId,
							workspaceId,
							paneIds,
							paneTitles: mergedTitles,
							cwd,
							rootPath: cwd,
							expandedPaths: new Set<string>(persisted?.expanded ?? []),
							selectedPath: primary,
							selectedPaths: primary ? new Set<string>([primary]) : new Set<string>(),
							anchorPath: primary,
							tree: null,
							loading: false,
						});
					}
				}

				const columnOrder = newColumns.map((c) => c.id);

				return {
					columns: newColumns,
					columnOrder,
					activeColumnId: state.activeColumnId,
				};
			});
		},

		/**
		 * Sync all workspaces at once, preserving state for every workspace.
		 * This is the keep-alive fix: even inactive workspaces get their columns
		 * updated, so switching back never loses pane→cwd associations.
		 *
		 * @param workspaces     All known workspaces.
		 * @param allPaneCwds    The full paneCwdStore snapshot (keys: "${wsId}:${paneId}").
		 * @param allPaneTitles  Optional full terminalTitles snapshot (keys: paneId).
		 */
		syncAllWorkspaces(
			workspaces: WorkspaceDescriptor[],
			allPaneCwds: Record<string, string>,
			allPaneTitles: Record<string, string> = {}
		): void {
			for (const ws of workspaces) {
				const paneCwds: Record<string, string> = {};
				const paneTitles: Record<string, string> = {};
				for (const [key, cwd] of Object.entries(allPaneCwds)) {
					if (key.startsWith(`${ws.id}:`)) {
						const paneId = key.slice(ws.id.length + 1);
						paneCwds[paneId] = cwd;
						if (allPaneTitles[paneId]) {
							paneTitles[paneId] = allPaneTitles[paneId];
						}
					}
				}
				this.syncWithPaneCwds(ws.id, paneCwds, paneTitles);
			}
		},

		/**
		 * Load file tree for a specific column.
		 *
		 * When `column.tree` is already populated we treat this as a silent
		 * background refresh: no `loading` flag is set so the Explorer body
		 * keeps rendering the cached tree and atomic-swaps when fresh data
		 * arrives. First-time loads (no cached tree) still set `loading`
		 * exactly once so the caller can show a subtle indicator if desired,
		 * but the body render path avoids a "加载中..." placeholder.
		 */
		async loadTree(columnId: string, depth = 3): Promise<void> {
			const state = get({ subscribe });
			const column = state.columns.find((c) => c.id === columnId);
			if (!column || column.loading) return;

			const isFirstLoad = !column.tree;
			update((s) => ({
				...s,
				columns: s.columns.map((c) =>
					c.id === columnId
						? { ...c, loading: isFirstLoad, needsRefresh: false }
						: c
				),
			}));

			try {
				if (!isTauri()) {
					const mockTree: FileNode = {
						name: column.cwd.split(/[/\\]/).pop() || 'root',
						path: column.cwd,
						is_dir: true,
						children: [
							{ name: 'src', path: `${column.cwd}/src`, is_dir: true, children: [] },
							{ name: 'package.json', path: `${column.cwd}/package.json`, is_dir: false },
							{ name: 'README.md', path: `${column.cwd}/README.md`, is_dir: false },
						],
					};
					update((s) => ({
						...s,
						columns: s.columns.map((c) =>
							c.id === columnId ? { ...c, tree: mockTree, loading: false } : c
						),
					}));
					return;
				}

				const tree = await invoke<FileNode>('get_file_tree', { path: column.cwd, depth });
				// Filter out ghost expanded paths (e.g. dirs deleted while we
				// weren't looking). Walk the fresh tree once, intersect with
				// persisted expanded set, and write the pruned set back so
				// localStorage doesn't keep growing with stale paths.
				const seen = new Set<string>();
				const walk = (n: FileNode) => {
					seen.add(n.path);
					if (n.children) for (const c of n.children) walk(c);
				};
				walk(tree);
				update((s) => ({
					...s,
					columns: s.columns.map((c) => {
						if (c.id !== columnId) return c;
						const pruned = new Set<string>();
						for (const p of c.expandedPaths) if (seen.has(p)) pruned.add(p);
						const nextCol = { ...c, tree, loading: false, expandedPaths: pruned };
						if (pruned.size !== c.expandedPaths.size) savePersistedColumn(nextCol);
						return nextCol;
					}),
				}));
			} catch (e) {
				const msg = String(e);
				// 路径不存在可能是 .ridge 恢复到新机器上的合法场景；其他错误仍可能是 bug。
				// 只 console.warn，不再给标题追加"· 目录不存在"文案 —— 避免用户在目录实际存在但
				// 路径传递有误（如正反斜杠 Windows 混用）的情况下被误导。
				const isMissing = /not exist|No such file|path is not a directory/i.test(msg);
				if (!isMissing) {
					reportDevIssue({ message: `Failed to load file tree: ${e}` });
				} else {
					console.warn('Explorer loadTree: missing path', column.cwd, msg);
				}
				const placeholder: FileNode = {
					name: column.cwd.split(/[/\\]/).pop() || 'root',
					path: column.cwd,
					is_dir: true,
					children: [],
				};
				update((s) => ({
					...s,
					columns: s.columns.map((c) =>
						c.id === columnId ? { ...c, tree: placeholder, loading: false } : c
					),
				}));
			}
		},

		/**
		 * Load children for an expanded directory.
		 */
		async loadChildren(columnId: string, dirPath: string): Promise<FileNode[]> {
			try {
				if (!isTauri()) {
					return [
						{ name: 'file1.ts', path: `${dirPath}/file1.ts`, is_dir: false },
						{ name: 'file2.ts', path: `${dirPath}/file2.ts`, is_dir: false },
					];
				}
				return await invoke<FileNode[]>('get_directory_children', { path: dirPath });
			} catch (e) {
				reportDevIssue({ message: `Failed to load directory children: ${e}` });
				return [];
			}
		},

		/**
		 * Expand multiple paths at once — a single store update + (implicitly)
		 * one localStorage write. Used by `Explorer.revealInTree` which needs
		 * to open a deep chain of ancestors without triggering N separate
		 * reactive updates (each of which would re-render the whole tree).
		 */
		expandMany(columnId: string, paths: string[]): void {
			if (paths.length === 0) return;
			update((state) => ({
				...state,
				columns: state.columns.map((c) => {
					if (c.id !== columnId) return c;
					const next = new Set(c.expandedPaths);
					let changed = false;
					for (const p of paths) {
						if (!next.has(p)) {
							next.add(p);
							changed = true;
						}
					}
					if (!changed) return c;
					const updated = { ...c, expandedPaths: next };
					savePersistedColumn(updated);
					return updated;
				}),
			}));
		},

		/**
		 * Toggle expanded state for a path in a column. Persists the column's
		 * new expandedPaths to localStorage so the tree shape survives a reload.
		 */
		toggleExpanded(columnId: string, path: string): void {
			update((state) => ({
				...state,
				columns: state.columns.map((c) => {
					if (c.id !== columnId) return c;
					const newExpanded = new Set(c.expandedPaths);
					if (newExpanded.has(path)) {
						newExpanded.delete(path);
					} else {
						newExpanded.add(path);
					}
					const next = { ...c, expandedPaths: newExpanded };
					savePersistedColumn(next);
					return next;
				}),
			}));
		},

		/**
		 * Set selected path for a column — single-selection shortcut.
		 * Replaces `selectedPaths` with `{path}`, resets anchor to `path`.
		 * Persisted (primary + expanded only; multi-select is session-scoped).
		 */
		setSelectedPath(columnId: string, path: string | null): void {
			update((state) => ({
				...state,
				columns: state.columns.map((c) => {
					if (c.id !== columnId) return c;
					const next: ExplorerColumn = {
						...c,
						selectedPath: path,
						selectedPaths: path ? new Set<string>([path]) : new Set<string>(),
						anchorPath: path,
					};
					savePersistedColumn(next);
					return next;
				}),
			}));
		},

		/**
		 * Full selection setter for multi-select paths (Shift/Ctrl interactions
		 * and Shift-Arrow keyboard extension). `primary` becomes `selectedPath`
		 * (the keyboard cursor); `anchor` defaults to `primary` and is only
		 * updated when a non-shift click happens (callers pass undefined on
		 * shift-range so the anchor stays pinned).
		 */
		setSelection(
			columnId: string,
			selection: {
				paths: Iterable<string>;
				primary: string | null;
				anchor?: string | null;
			}
		): void {
			update((state) => ({
				...state,
				columns: state.columns.map((c) => {
					if (c.id !== columnId) return c;
					const paths = new Set<string>(selection.paths);
					if (selection.primary) paths.add(selection.primary);
					const next: ExplorerColumn = {
						...c,
						selectedPath: selection.primary,
						selectedPaths: paths,
						anchorPath:
							selection.anchor === undefined ? c.anchorPath : selection.anchor,
					};
					savePersistedColumn(next);
					return next;
				}),
			}));
		},

		/**
		 * Set active column.
		 */
		setActiveColumn(columnId: string | null): void {
			update((state) => ({ ...state, activeColumnId: columnId }));
		},

		/**
		 * Reorder columns (drag-and-drop).
		 */
		reorderColumns(fromIndex: number, toIndex: number): void {
			update((state) => {
				if (fromIndex === toIndex) return state;
				const columns = [...state.columns];
				const [moved] = columns.splice(fromIndex, 1);
				columns.splice(toIndex, 0, moved);
				const columnOrder = columns.map((c) => c.id);
				return { ...state, columns, columnOrder };
			});
		},

		/**
		 * Remove column.
		 */
		removeColumn(columnId: string): void {
			update((state) => ({
				...state,
				columns: state.columns.filter((c) => c.id !== columnId),
				columnOrder: state.columnOrder.filter((id) => id !== columnId),
				activeColumnId:
					state.activeColumnId === columnId
						? state.columns[0]?.id || null
						: state.activeColumnId,
			}));
		},

		/**
		 * Clear all columns for a workspace.
		 */
		clearWorkspace(workspaceId: string): void {
			update((state) => ({
				...state,
				columns: state.columns.filter((c) => c.workspaceId !== workspaceId),
				columnOrder: state.columnOrder.filter(
					(id) => !state.columns.find((c) => c.id === id && c.workspaceId === workspaceId)
				),
				activeColumnId: state.columns.find((c) => c.workspaceId === workspaceId)
					? state.activeColumnId
					: null,
			}));
		},

		/**
		 * Reset store.
		 */
		reset(): void {
			set(initialState);
		},
	};
}

export const fileExplorerStore = createFileExplorerStore();

// Derived stores for convenience
export const explorerColumns = derived(fileExplorerStore, ($s) => $s.columns);
export const explorerColumnOrder = derived(fileExplorerStore, ($s) => $s.columnOrder);
export const activeExplorerColumn = derived(fileExplorerStore, ($s) =>
	$s.columns.find((c) => c.id === $s.activeColumnId) || null
);

// ---- Internal workspace name registry (kept in sync by initFileExplorer / syncAllWorkspaces) ----
const _workspaceNames = writable<Record<string, string>>({});

/** Update the workspace name registry used by the grouped derived store. */
export function updateWorkspaceNames(workspaces: WorkspaceDescriptor[]): void {
	const map: Record<string, string> = {};
	for (const ws of workspaces) {
		map[ws.id] = ws.name || `工作区 ${ws.index + 1}`;
	}
	_workspaceNames.set(map);
}

// ---- Collapsed state for workspace groups (persists across renders) ----
const _collapsedWorkspaces = writable<Set<string>>(new Set());

export function toggleWorkspaceCollapsed(workspaceId: string): void {
	_collapsedWorkspaces.update((s) => {
		const next = new Set(s);
		if (next.has(workspaceId)) next.delete(workspaceId);
		else next.add(workspaceId);
		return next;
	});
}

// ---- Collapsed state for individual cwd columns (T18：三层节点的中间层) ----
//
// 资源管理器三层结构：工作区 → 终端节点（cwd column）→ 文件树。
// 终端节点（即一个 cwd 下合并若干同 cwd 终端 pane 的卡片）允许独立折叠，
// 折叠时隐藏文件树仅保留头部（cwd 路径 + pane 标签）。持久化到 localStorage
// 让用户切回 Ridge 后保留之前的折叠形状。
const COLLAPSED_COLUMNS_KEY = 'ridge-explorer-collapsed-columns';

function loadCollapsedColumns(): Set<string> {
	if (typeof localStorage === 'undefined') return new Set();
	try {
		const raw = localStorage.getItem(COLLAPSED_COLUMNS_KEY);
		if (!raw) return new Set();
		const arr = JSON.parse(raw);
		return Array.isArray(arr) ? new Set(arr.filter((s: unknown) => typeof s === 'string')) : new Set();
	} catch {
		return new Set();
	}
}

function persistCollapsedColumns(s: Set<string>): void {
	if (typeof localStorage === 'undefined') return;
	try {
		localStorage.setItem(COLLAPSED_COLUMNS_KEY, JSON.stringify(Array.from(s)));
	} catch {
		/* ignore */
	}
}

const _collapsedColumns = writable<Set<string>>(loadCollapsedColumns());
export const collapsedColumns = { subscribe: _collapsedColumns.subscribe };

export function toggleColumnCollapsed(columnId: string): void {
	_collapsedColumns.update((s) => {
		const next = new Set(s);
		if (next.has(columnId)) next.delete(columnId);
		else next.add(columnId);
		persistCollapsedColumns(next);
		return next;
	});
}

/**
 * Grouped derived store: emits an array of ExplorerWorkspaceGroup sorted by
 * the order workspaces appear in the workspace list.
 */
export const explorerWorkspaceGroups = derived(
	[fileExplorerStore, _workspaceNames, _collapsedWorkspaces],
	([$s, $names, $collapsed]) => {
		// Collect unique workspace ids in the order they appear in columns
		const seenOrder: string[] = [];
		const seenSet = new Set<string>();
		for (const col of $s.columns) {
			if (!seenSet.has(col.workspaceId)) {
				seenOrder.push(col.workspaceId);
				seenSet.add(col.workspaceId);
			}
		}

		const groups: ExplorerWorkspaceGroup[] = seenOrder.map((wsId) => ({
			workspaceId: wsId,
			workspaceName: $names[wsId] || wsId,
			columns: $s.columns.filter((c) => c.workspaceId === wsId),
			collapsed: $collapsed.has(wsId),
		}));

		return groups;
	}
);

// ─── Clipboard state (cut/copy/paste between Explorer rows) ────────────────
// Session-scoped — not persisted. Matches VS Code's Explorer clipboard which
// drops on window close. `mode === 'cut'` gives us a dimmed visual on the
// source rows until paste consumes it or the user cancels with Escape.
export interface ExplorerClipboard {
	paths: string[];
	mode: 'copy' | 'cut';
}

const _clipboard = writable<ExplorerClipboard | null>(null);
export const explorerClipboard = { subscribe: _clipboard.subscribe };

export function setExplorerClipboard(clip: ExplorerClipboard | null): void {
	_clipboard.set(clip);
}

/**
 * Derive a non-colliding child name inside `dirPath` by appending "(N)" before
 * the extension. Used by the paste + drag-drop code paths so both surfaces
 * share identical conflict-avoidance semantics — VS Code's rename style.
 *
 * `existingAbsolute` is a set of *absolute* paths already present in the
 * directory; we join `dirPath + desired` using the dir's own separator style
 * and test membership.
 */
export function uniqueChildName(
	dirPath: string,
	desired: string,
	existingAbsolute: Set<string>
): string {
	const sep = dirPath.includes('\\') && !dirPath.includes('/') ? '\\' : '/';
	const clean = dirPath.replace(/[\\/]+$/, '');
	const dotIdx = desired.lastIndexOf('.');
	const base = dotIdx > 0 ? desired.slice(0, dotIdx) : desired;
	const ext = dotIdx > 0 ? desired.slice(dotIdx) : '';
	if (!existingAbsolute.has(`${clean}${sep}${desired}`)) return desired;
	for (let i = 1; i < 999; i += 1) {
		const candidate = `${base} (${i})${ext}`;
		if (!existingAbsolute.has(`${clean}${sep}${candidate}`)) return candidate;
	}
	return `${base} (${Date.now()})${ext}`;
}

/**
 * Walk all known columns' cached trees; reload the tree of any column whose
 * cached set contains `dirPath`. Called after a mutation (paste, drop, delete)
 * so every view of a directory that might be affected refreshes — e.g. two
 * workspaces both at the same cwd, or a parent column whose tree cached the
 * mutated subdirectory.
 */
export async function refreshColumnsCovering(dirPath: string): Promise<void> {
	const state = get(fileExplorerStore);
	const contains = (node: FileNode | null): boolean => {
		if (!node) return false;
		if (node.path === dirPath) return true;
		if (!node.children) return false;
		for (const c of node.children) if (contains(c)) return true;
		return false;
	};
	await Promise.all(
		state.columns
			.filter((c) => contains(c.tree))
			.map((c) => fileExplorerStore.loadTree(c.id))
	);
}

/**
 * Flatten an ExplorerColumn's tree to the list of currently-visible node
 * paths (root first, then expanded children in order). Used by the Explorer's
 * root-level ArrowUp/Down handler to compute prev/next selection targets.
 */
export function flattenVisiblePaths(column: ExplorerColumn): string[] {
	if (!column.tree) return [];
	const result: string[] = [];
	const walk = (node: FileNode) => {
		result.push(node.path);
		if (node.is_dir && column.expandedPaths.has(node.path) && node.children) {
			for (const child of node.children) walk(child);
		}
	};
	// Skip root — Explorer renders col.tree.children directly (top-level folder layer removed)
	for (const child of column.tree.children ?? []) walk(child);
	return result;
}

/**
 * Initialize file explorer for all known workspaces (keep-alive fix).
 * Should be called when switching to the files tab or when workspaces change.
 * Previously only synced the active workspace, causing inactive workspace
 * state to be forgotten on switch.
 */
export function initFileExplorer(
	workspaces: WorkspaceDescriptor[],
	allPaneTitles: Record<string, string> = {}
): void {
	const cwds = get(paneCwdStore);

	// Update the name registry so the grouped derived store has correct names
	updateWorkspaceNames(workspaces);

	// Sync every workspace (not just the active one)
	fileExplorerStore.syncAllWorkspaces(workspaces, cwds, allPaneTitles);

	// Load trees for columns that don't have data yet
	const state = get(fileExplorerStore);
	for (const col of state.columns) {
		if (!col.tree && !col.loading) {
			void fileExplorerStore.loadTree(col.id);
		}
	}
}
