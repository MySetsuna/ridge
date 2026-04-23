<script lang="ts">
	import { ChevronRight, X, RefreshCw, FolderOpen } from 'lucide-svelte';
	import {
		fileExplorerStore,
		initFileExplorer,
		explorerWorkspaceGroups,
		toggleWorkspaceCollapsed,
		updateWorkspaceNames,
	} from '$lib/stores/fileExplorer';
	import { paneCwdStore, terminalTitles, workspacesList, activeWorkspaceId } from '$lib/stores/paneTree';
	import { get } from 'svelte/store';
	import FileTree from './FileTree.svelte';

	interface Props {
		workspaceId: string;
	}

	let { workspaceId }: Props = $props();

	/** Per-column collapse state (separate from workspace-level collapse). */
	let collapsedColumns = $state(new Set<string>());

	// --- Initial sync: all workspaces ---
	$effect(() => {
		const wsList = $workspacesList;
		const titles = $terminalTitles;
		if (wsList.length > 0) {
			initFileExplorer(wsList, titles);
		}
	});

	// --- Reactive sync: re-run whenever any pane cwd changes ---
	$effect(() => {
		const cwds = $paneCwdStore;
		const titles = $terminalTitles;
		const wsList = $workspacesList;

		// Update workspace names (handles renames)
		updateWorkspaceNames(wsList);

		// Sync every workspace to keep inactive ones alive
		for (const ws of wsList) {
			const workspaceCwds: Record<string, string> = {};
			const workspaceTitles: Record<string, string> = {};
			for (const [key, cwd] of Object.entries(cwds)) {
				if (key.startsWith(`${ws.id}:`)) {
					const paneId = key.slice(ws.id.length + 1);
					workspaceCwds[paneId] = cwd;
					if (titles[paneId]) {
						workspaceTitles[paneId] = titles[paneId];
					}
				}
			}
			fileExplorerStore.syncWithPaneCwds(ws.id, workspaceCwds, workspaceTitles);
		}

		// Trigger tree loads for any new columns
		const cols = get(fileExplorerStore).columns;
		for (const col of cols) {
			if (!col.tree && !col.loading) {
				void fileExplorerStore.loadTree(col.id);
			}
		}
	});

	function toggleColumnCollapse(columnId: string) {
		const next = new Set(collapsedColumns);
		if (next.has(columnId)) {
			next.delete(columnId);
		} else {
			next.add(columnId);
		}
		collapsedColumns = next;
	}

	function handleCloseColumn(columnId: string) {
		fileExplorerStore.removeColumn(columnId);
	}

	function handleRefresh(columnId: string) {
		void fileExplorerStore.loadTree(columnId);
	}

	function handleFileSelect(path: string, columnId: string) {
		console.log('Selected:', path, 'in column:', columnId);
	}

	function cwdBasename(cwd: string): string {
		return cwd.split(/[/\\]/).filter(Boolean).pop() || cwd;
	}

	function getPaneLabel(paneId: string, paneTitles: Record<string, string>): string {
		return paneTitles[paneId] || $terminalTitles[paneId] || '终端';
	}

	const totalColumns = $derived($explorerWorkspaceGroups.reduce((n, g) => n + g.columns.length, 0));
</script>

<div class="explorer flex h-full flex-col overflow-y-auto" data-testid="file-tree">
	{#if totalColumns === 0}
		<div class="flex-1 flex items-center justify-center">
			<div class="text-center">
				<FolderOpen class="mx-auto h-12 w-12 text-[var(--wf-fg-muted)] mb-4" />
				<p class="text-[13px] text-[var(--wf-fg-muted)]">无活动终端</p>
				<p class="text-[12px] text-[var(--wf-fg-muted)] mt-1">
					打开终端后将在此显示文件树
				</p>
			</div>
		</div>
	{:else}
		{#each $explorerWorkspaceGroups as group (group.workspaceId)}
			<!-- ══ Workspace header row ══ -->
			<div
				class="explorer-workspace border-b border-[var(--wf-border)] last:border-b-0"
			>
				<div
					class="group/ws flex items-center h-8 px-2 gap-1.5 cursor-pointer select-none
						{group.workspaceId === $activeWorkspaceId
							? 'bg-[var(--wf-accent)]/10 text-[var(--wf-fg)]'
							: 'text-[var(--wf-fg-muted)] hover:bg-[var(--wf-surface)]/40'}"
					role="button"
					tabindex="0"
					onclick={() => toggleWorkspaceCollapsed(group.workspaceId)}
					onkeydown={(e) => e.key === 'Enter' && toggleWorkspaceCollapsed(group.workspaceId)}
				>
					<!-- Workspace collapse chevron -->
					<ChevronRight
						class="h-3.5 w-3.5 shrink-0 transition-transform duration-150
							{group.collapsed ? '' : 'rotate-90'}
							{group.workspaceId === $activeWorkspaceId ? 'text-[var(--wf-accent)]' : 'text-[var(--wf-fg-muted)]'}"
					/>

					<!-- Workspace name -->
					<span
						class="flex-1 text-[11px] font-semibold uppercase tracking-wider truncate
							{group.workspaceId === $activeWorkspaceId ? 'text-[var(--wf-accent)]' : 'text-[var(--wf-fg-muted)]'}"
					>
						{group.workspaceName}
					</span>

					<!-- Column count badge -->
					<span class="text-[10px] text-[var(--wf-fg-muted)] shrink-0">
						{group.columns.length}
					</span>
				</div>

				<!-- ══ CWD groups under this workspace ══ -->
				{#if !group.collapsed}
					{#each group.columns as col (col.id)}
						<div class="explorer-section border-t border-[var(--wf-border)]/50">
							<!-- Compact CWD section header -->
							<div
								class="group flex items-center h-7 pl-5 pr-2 gap-1 cursor-pointer select-none hover:bg-[var(--wf-surface)]/60 transition-colors"
								role="button"
								tabindex="0"
								onclick={() => toggleColumnCollapse(col.id)}
								onkeydown={(e) => e.key === 'Enter' && toggleColumnCollapse(col.id)}
							>
								<!-- Collapse arrow -->
								<ChevronRight
									class="h-3 w-3 shrink-0 text-[var(--wf-fg-muted)] transition-transform duration-150 {collapsedColumns.has(col.id) ? '' : 'rotate-90'}"
								/>

								<!-- Pane name badges (terminals sharing this CWD) -->
								<div class="flex items-center gap-1 min-w-0 flex-1">
									{#each col.paneIds as pid (pid)}
										<span
											class="inline-flex items-center px-1.5 py-0 rounded text-[10px] font-medium bg-[var(--wf-surface)] text-[var(--wf-fg)] border border-[var(--wf-border)] truncate max-w-[80px]"
											title={getPaneLabel(pid, col.paneTitles)}
										>
											{getPaneLabel(pid, col.paneTitles)}
										</span>
									{/each}
								</div>

								<!-- CWD basename -->
								<span
									class="text-[10px] text-[var(--wf-fg-muted)] truncate max-w-[100px] shrink-0"
									title={col.cwd}
								>
									{cwdBasename(col.cwd)}
								</span>

								<!-- Loading spinner or refresh button -->
								{#if col.loading}
									<RefreshCw class="h-3 w-3 shrink-0 animate-spin text-[var(--wf-accent)]" />
								{:else}
									<button
										type="button"
										class="flex h-5 w-5 shrink-0 items-center justify-center rounded text-[var(--wf-fg-muted)] opacity-0 group-hover:opacity-60 hover:!opacity-100 hover:bg-[var(--wf-accent)]/20 hover:text-[var(--wf-fg)] transition-all"
										onclick={(e) => {
											e.stopPropagation();
											handleRefresh(col.id);
										}}
										title="刷新"
									>
										<RefreshCw class="h-3 w-3" />
									</button>
								{/if}

								<!-- Close button -->
								<button
									type="button"
									class="flex h-5 w-5 shrink-0 items-center justify-center rounded text-[var(--wf-fg-muted)] opacity-0 group-hover:opacity-60 hover:!opacity-100 hover:bg-red-500/20 hover:text-red-400 transition-all"
									onclick={(e) => {
										e.stopPropagation();
										handleCloseColumn(col.id);
									}}
									title="关闭"
								>
									<X class="h-3 w-3" />
								</button>
							</div>

							<!-- Collapsible file tree body -->
							{#if !collapsedColumns.has(col.id)}
								<div class="explorer-body py-0.5">
									{#if col.tree}
										<FileTree
											columnId={col.id}
											node={col.tree}
											depth={0}
											expandedPaths={col.expandedPaths}
											selectedPath={col.selectedPath}
											onSelect={(path) => handleFileSelect(path, col.id)}
										/>
									{:else if col.loading}
										<div class="px-4 py-2 text-[11px] text-[var(--wf-fg-muted)]">加载中...</div>
									{:else}
										<div class="px-4 py-2 text-[11px] text-[var(--wf-fg-muted)]">
											空目录
										</div>
									{/if}
								</div>
							{/if}
						</div>
					{/each}
				{/if}
			</div>
		{/each}
	{/if}
</div>

<style>
	.explorer::-webkit-scrollbar {
		width: 4px;
	}

	.explorer::-webkit-scrollbar-track {
		background: transparent;
	}

	.explorer::-webkit-scrollbar-thumb {
		background: var(--wf-border);
		border-radius: 2px;
	}

	.explorer::-webkit-scrollbar-thumb:hover {
		background: var(--wf-fg-muted);
	}
</style>
