<script lang="ts">
	import { GripVertical, X, FolderOpen, Plus, RefreshCw } from 'lucide-svelte';
	import { onMount } from 'svelte';
	import { fileExplorerStore, initFileExplorer, explorerColumns } from '$lib/stores/fileExplorer';
	import { activeWorkspaceId, paneCwdStore } from '$lib/stores/paneTree';
	import FileTree from './FileTree.svelte';

	interface Props {
		workspaceId: string;
	}

	let { workspaceId }: Props = $props();

	let draggingIndex: number | null = $state(null);
	let dragOverIndex: number | null = $state(null);

	// Initialize when workspace changes or component mounts
	$effect(() => {
		if (workspaceId) {
			initFileExplorer(workspaceId);
		}
	});

	// React to paneCwdStore changes
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

	// Load trees when columns change
	$effect(() => {
		const columns = $explorerColumns;
		for (const col of columns) {
			if (!col.tree && !col.loading) {
				fileExplorerStore.loadTree(col.id);
			}
		}
	});

	function handleDragStart(e: DragEvent, index: number) {
		draggingIndex = index;
		if (e.dataTransfer) {
			e.dataTransfer.effectAllowed = 'move';
			e.dataTransfer.setData('text/plain', index.toString());
		}
	}

	function handleDragOver(e: DragEvent, index: number) {
		e.preventDefault();
		dragOverIndex = index;
	}

	function handleDragLeave() {
		dragOverIndex = null;
	}

	function handleDrop(e: DragEvent, index: number) {
		e.preventDefault();
		if (draggingIndex !== null && draggingIndex !== index) {
			fileExplorerStore.reorderColumns(draggingIndex, index);
		}
		draggingIndex = null;
		dragOverIndex = null;
	}

	function handleDragEnd() {
		draggingIndex = null;
		dragOverIndex = null;
	}

	function handleCloseColumn(columnId: string) {
		fileExplorerStore.removeColumn(columnId);
	}

	function handleRefresh(columnId: string) {
		fileExplorerStore.loadTree(columnId);
	}

	function handleFileSelect(path: string, columnId: string) {
		console.log('Selected:', path, 'in column:', columnId);
		// Could emit an event or open the file
	}

	function getPaneTitle(paneId: string): string {
		return `终端 ${paneId.slice(0, 6)}`;
	}
</script>

<div class="explorer flex h-full flex-col">
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
		<!-- Column headers with drag handles -->
		<div class="explorer-headers flex shrink-0 border-b border-[var(--wf-border)] overflow-x-auto">
			{#each $explorerColumns as col, i (col.id)}
				<div
					class="explorer-header group relative flex items-center gap-2 border-r border-[var(--wf-border)] px-3 py-2 min-w-[180px] max-w-[280px] cursor-grab active:cursor-grabbing {dragOverIndex ===
					i
						? 'bg-[var(--wf-accent)]/20 ring-2 ring-[var(--wf-accent)]/50'
						: ''}"
					draggable="true"
					ondragstart={(e) => handleDragStart(e, i)}
					ondragover={(e) => handleDragOver(e, i)}
					ondragleave={handleDragLeave}
					ondrop={(e) => handleDrop(e, i)}
					ondragend={handleDragEnd}
					role="button"
					tabindex="0"
				>
					<!-- Drag handle -->
					<GripVertical
						class="h-4 w-4 shrink-0 text-[var(--wf-fg-muted)] opacity-40 group-hover:opacity-70"
					/>

					<!-- Column title -->
					<div class="flex-1 min-w-0">
						<div
							class="text-[12px] font-medium text-[var(--wf-fg)] truncate"
							title={col.cwd}
						>
							{getPaneTitle(col.paneId)}
						</div>
						<div class="text-[10px] text-[var(--wf-fg-muted)] truncate" title={col.cwd}>
							{col.cwd}
						</div>
					</div>

					<!-- Loading indicator -->
					{#if col.loading}
						<RefreshCw class="h-3.5 w-3.5 animate-spin text-[var(--wf-accent)]" />
					{/if}

					<!-- Close button -->
					<button
						type="button"
						class="h-5 w-5 flex items-center justify-center rounded text-[var(--wf-fg-muted)] opacity-0 group-hover:opacity-100 hover:bg-[var(--wf-accent)]/20 hover:text-[var(--wf-fg)] transition-all"
						onclick={(e) => {
							e.stopPropagation();
							handleCloseColumn(col.id);
						}}
						title="关闭"
					>
						<X class="h-3.5 w-3.5" />
					</button>
				</div>
			{/each}
		</div>

		<!-- Column bodies -->
		<div class="explorer-bodies flex-1 flex overflow-hidden">
			{#each $explorerColumns as col (col.id)}
				<div
					class="explorer-body flex-1 min-w-0 overflow-auto border-r border-[var(--wf-border)] last:border-r-0 p-1"
				>
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
						<div class="p-2 text-[12px] text-[var(--wf-fg-muted)]">加载中...</div>
					{:else}
						<div class="p-2 text-[12px] text-[var(--wf-fg-muted)]">
							点击刷新按钮加载文件树
						</div>
					{/if}
				</div>
			{/each}
		</div>
	{/if}
</div>

<style>
	.explorer::-webkit-scrollbar {
		height: 6px;
		width: 6px;
	}

	.explorer::-webkit-scrollbar-track {
		background: transparent;
	}

	.explorer::-webkit-scrollbar-thumb {
		background: var(--wf-border);
		border-radius: 3px;
	}

	.explorer::-webkit-scrollbar-thumb:hover {
		background: var(--wf-fg-muted);
	}
</style>