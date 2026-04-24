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
	selectedPath: string | null;
	tree: FileNode | null;
	loading: boolean;
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
						// 重进老目录时，缓存的 tree 多半是过时的（此期间用户可能在终端里 `touch`、`rm`），
						// 置 tree=null 让 Explorer effect 自动触发一次重载，避免"切回来看到旧文件"。
						const prevSet = new Set(existing.paneIds);
						const hasNewJoiner = paneIds.some((id) => !prevSet.has(id));
						newColumns.push({
							...existing,
							paneIds,
							paneTitles: mergedTitles,
							tree: hasNewJoiner ? null : existing.tree,
						});
					} else {
						// New CWD: create column, tree: null triggers load
						newColumns.push({
							id: colId,
							workspaceId,
							paneIds,
							paneTitles: mergedTitles,
							cwd,
							rootPath: cwd,
							expandedPaths: new Set<string>(),
							selectedPath: null,
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
		 */
		async loadTree(columnId: string, depth = 3): Promise<void> {
			const state = get({ subscribe });
			const column = state.columns.find((c) => c.id === columnId);
			if (!column || column.loading) return;

			update((s) => ({
				...s,
				columns: s.columns.map((c) => (c.id === columnId ? { ...c, loading: true } : c)),
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
				update((s) => ({
					...s,
					columns: s.columns.map((c) => (c.id === columnId ? { ...c, tree, loading: false } : c)),
				}));
			} catch (e) {
				reportDevIssue({ message: `Failed to load file tree: ${e}` });
				update((s) => ({
					...s,
					columns: s.columns.map((c) => (c.id === columnId ? { ...c, loading: false } : c)),
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
		 * Toggle expanded state for a path in a column.
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
					return { ...c, expandedPaths: newExpanded };
				}),
			}));
		},

		/**
		 * Set selected path for a column.
		 */
		setSelectedPath(columnId: string, path: string | null): void {
			update((state) => ({
				...state,
				columns: state.columns.map((c) =>
					c.id === columnId ? { ...c, selectedPath: path } : c
				),
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
