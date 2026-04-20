// src/lib/stores/fileExplorer.ts
import { writable, get, derived, type Writable } from 'svelte/store';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { getDirectoryChildren, refreshFileTree, type FileNode } from './project';
import { paneCwdStore, activeWorkspaceId } from './paneTree';
import { reportDevIssue } from '$lib/devIssue';

export interface ExplorerColumn {
	id: string; // "${workspaceId}:${paneId}"
	workspaceId: string;
	paneId: string;
	cwd: string;
	rootPath: string;
	expandedPaths: Set<string>;
	selectedPath: string | null;
	tree: FileNode | null;
	loading: boolean;
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

function createFileExplorerStore() {
	const { subscribe, set, update } = writable<FileExplorerState>(initialState);

	return {
		subscribe,

		/**
		 * Sync columns with pane CWDs for the active workspace.
		 * Adds new columns for new CWDs and removes columns for deleted panes.
		 */
		syncWithPaneCwds(workspaceId: string, paneCwds: Record<string, string>): void {
			update((state) => {
				const existingIds = new Set(state.columns.map((c) => c.id));
				const newColumns: ExplorerColumn[] = [];

				// Keep existing columns
				for (const col of state.columns) {
					if (col.workspaceId === workspaceId && paneCwds[col.paneId] !== undefined) {
						// Update cwd if changed
						if (col.cwd !== paneCwds[col.paneId]) {
							newColumns.push({ ...col, cwd: paneCwds[col.paneId], rootPath: paneCwds[col.paneId] });
						} else {
							newColumns.push(col);
						}
					}
				}

				// Add new columns for new pane CWDs
				for (const [paneId, cwd] of Object.entries(paneCwds)) {
					const colId = `${workspaceId}:${paneId}`;
					if (!existingIds.has(colId)) {
						newColumns.push({
							id: colId,
							workspaceId,
							paneId,
							cwd,
							rootPath: cwd,
							expandedPaths: new Set<string>(),
							selectedPath: null,
							tree: null,
							loading: true,
						});
					}
				}

				// Update columnOrder to match columns order
				const columnOrder = newColumns.map((c) => c.id);

				return {
					columns: newColumns,
					columnOrder,
					activeColumnId: state.activeColumnId,
				};
			});
		},

		/**
		 * Load file tree for a specific column.
		 */
		async loadTree(columnId: string, depth = 3): Promise<void> {
			const state = get({ subscribe });
			const column = state.columns.find((c) => c.id === columnId);
			if (!column) return;

			update((s) => ({
				...s,
				columns: s.columns.map((c) => (c.id === columnId ? { ...c, loading: true } : c)),
			}));

			try {
				if (!isTauri()) {
					// Mock data for non-Tauri environment
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
					// Mock data for non-Tauri
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

/**
 * Initialize file explorer for the active workspace.
 * Should be called when switching to the files tab or when workspace changes.
 */
export function initFileExplorer(workspaceId: string): void {
	const cwds = get(paneCwdStore);
	const workspaceCwds: Record<string, string> = {};

	for (const [key, cwd] of Object.entries(cwds)) {
		if (key.startsWith(`${workspaceId}:`)) {
			const paneId = key.slice(workspaceId.length + 1);
			workspaceCwds[paneId] = cwd;
		}
	}

	fileExplorerStore.syncWithPaneCwds(workspaceId, workspaceCwds);

	// Load trees for all columns
	const state = get(fileExplorerStore);
	for (const col of state.columns) {
		fileExplorerStore.loadTree(col.id);
	}
}