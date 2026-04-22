<script lang="ts">
	import { ChevronRight, X, RefreshCw, FolderOpen } from 'lucide-svelte';
	import { fileExplorerStore, initFileExplorer, explorerColumns } from '$lib/stores/fileExplorer';
	import { activeWorkspaceId, paneCwdStore } from '$lib/stores/paneTree';
	import FileTree from './FileTree.svelte';

	interface Props {
		workspaceId: string;
	}

	let { workspaceId }: Props = $props();

	let collapsedColumns = $state(new Set<string>());

	$effect(() => {
		if (workspaceId) {
			initFileExplorer(workspaceId);
		}
	});

	$effect(() => {
		const cwds = $paneCwdStore;
		const workspaceCwds: Record<string, string> = {};
		for (const [key, cwd] of Object.entries(cwds)) {
			if (key.startsWith(`${workspaceId}:`)) {
				const paneId = key.slice(workspaceId.length + 1);
				workspaceCwds[paneId] = cwd;
			}
		}
		fileExplorerStore.syncWithPaneCwds(workspaceId, workspaceCwds);
	});

	$effect(() => {
		const columns = $explorerColumns;
		for (const col of columns) {
			if (!col.tree && !col.loading) {
				fileExplorerStore.loadTree(col.id);
			}
		}
	});

	function toggleCollapse(columnId: string) {
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
		fileExplorerStore.loadTree(columnId);
	}

	function handleFileSelect(path: string, columnId: string) {
		console.log('Selected:', path, 'in column:', columnId);
	}

	function cwdBasename(cwd: string): string {
		return cwd.split(/[/\\]/).filter(Boolean).pop() || cwd;
	}

	function getPaneTitle(paneId: string): string {
		return `终端 ${paneId.slice(0, 6)}`;
	}
</script>

<div class="explorer flex h-full flex-col overflow-y-auto" data-testid="file-tree">
	{#if $explorerColumns.length === 0}
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
		{#each $explorerColumns as col (col.id)}
			<div class="explorer-section border-b border-[var(--wf-border)] last:border-b-0">
				<!-- Compact section header -->
				<div
					class="group flex items-center h-7 px-2 gap-1 cursor-pointer select-none hover:bg-[var(--wf-surface)]/60 transition-colors"
					role="button"
					tabindex="0"
					onclick={() => toggleCollapse(col.id)}
					onkeydown={(e) => e.key === 'Enter' && toggleCollapse(col.id)}
				>
					<!-- Collapse arrow -->
					<ChevronRight
						class="h-3 w-3 shrink-0 text-[var(--wf-fg-muted)] transition-transform duration-150 {collapsedColumns.has(col.id) ? '' : 'rotate-90'}"
					/>

					<!-- Terminal title -->
					<span class="text-[11px] font-medium text-[var(--wf-fg)] truncate flex-1 min-w-0">
						{getPaneTitle(col.paneId)}
					</span>

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
