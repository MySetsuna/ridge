<script lang="ts">
	import {
		Folder,
		FolderOpen,
		File,
		ChevronRight,
		ChevronDown,
		FileCode,
		FileText,
		Image,
	} from 'lucide-svelte';
	import { showContextMenu } from '$lib/stores/contextMenu';
	import { fileExplorerStore, type ExplorerColumn } from '$lib/stores/fileExplorer';
	import type { FileNode } from '$lib/stores/project';
	import FileTree from './FileTree.svelte';

	interface Props {
		columnId: string;
		node: FileNode;
		depth?: number;
		expandedPaths?: Set<string>;
		selectedPath?: string | null;
		onSelect?: (path: string) => void;
	}

	let { columnId, node, depth = 0, expandedPaths = new Set(), selectedPath = null, onSelect }: Props =
		$props();

	let children = $state<FileNode[]>([]);
	let loading = $state(false);
	let hasLoaded = $state(false);

	let isExpanded = $derived(expandedPaths.has(node.path));
	let isSelected = $derived(selectedPath === node.path);

	// Load children when expanding
	$effect(() => {
		if (isExpanded && node.is_dir && !hasLoaded) {
			loadChildren();
		}
	});

	async function loadChildren() {
		if (!node.is_dir) return;
		loading = true;
		try {
			children = await fileExplorerStore.loadChildren(columnId, node.path);
			hasLoaded = true;
		} catch (e) {
			console.error('Failed to load children:', e);
		} finally {
			loading = false;
		}
	}

	function handleClick() {
		if (node.is_dir) {
			fileExplorerStore.toggleExpanded(columnId, node.path);
		}
		if (onSelect) {
			onSelect(node.path);
		}
		fileExplorerStore.setSelectedPath(columnId, node.path);
	}

	function handleContextMenu(e: MouseEvent) {
		e.preventDefault();
		e.stopPropagation();

		const items = node.is_dir
			? [
					{ id: 'new-file', label: '新建文件', action: () => createFile() },
					{ id: 'new-folder', label: '新建文件夹', action: () => createFolder() },
					{ id: 'divider1', divider: true },
					{ id: 'open', label: '在终端中打开', action: () => openInTerminal() },
					{ id: 'reveal', label: '在文件管理器中显示', action: () => revealInExplorer() },
					{ id: 'divider2', divider: true },
					{ id: 'rename', label: '重命名', action: () => rename() },
					{ id: 'delete', label: '删除', action: () => deleteItem() },
				]
			: [
					{ id: 'open', label: '打开', action: () => openFile() },
					{ id: 'reveal', label: '在文件管理器中显示', action: () => revealInExplorer() },
					{ id: 'divider', divider: true },
					{ id: 'rename', label: '重命名', action: () => rename() },
					{ id: 'delete', label: '删除', action: () => deleteItem() },
				];

		showContextMenu(e.clientX, e.clientY, items);
	}

	function createFile() {
		console.log('Create file in', node.path);
	}

	function createFolder() {
		console.log('Create folder in', node.path);
	}

	function openInTerminal() {
		console.log('Open in terminal', node.path);
	}

	function revealInExplorer() {
		console.log('Reveal in explorer', node.path);
	}

	function openFile() {
		console.log('Open file', node.path);
	}

	function rename() {
		console.log('Rename', node.path);
	}

	function deleteItem() {
		console.log('Delete', node.path);
	}

	function getFileIcon(name: string) {
		const ext = name.split('.').pop()?.toLowerCase();
		if (['ts', 'tsx', 'js', 'jsx', 'svelte', 'rs', 'py', 'go'].includes(ext || '')) {
			return FileCode;
		}
		if (['md', 'txt', 'json', 'yaml', 'yml', 'toml'].includes(ext || '')) {
			return FileText;
		}
		if (['png', 'jpg', 'jpeg', 'gif', 'svg', 'webp'].includes(ext || '')) {
			return Image;
		}
		return File;
	}
</script>

<div class="file-tree-node" style="padding-left: {depth * 16}px">
	<button
		type="button"
		class="flex w-full items-center gap-1.5 px-2 py-1 text-left text-[13px] transition-colors hover:bg-[var(--wf-accent)]/10 {isSelected
			? 'bg-[var(--wf-accent)]/20 text-[var(--wf-accent)]'
			: 'text-[var(--wf-fg)]'}"
		onclick={handleClick}
		oncontextmenu={handleContextMenu}
	>
		{#if node.is_dir}
			<span class="w-4 h-4 flex items-center justify-center shrink-0 text-[var(--wf-fg-muted)]">
				{#if isExpanded}
					<ChevronDown size={14} />
				{:else}
					<ChevronRight size={14} />
				{/if}
			</span>
			<span class="w-4 h-4 flex items-center justify-center shrink-0 text-[var(--wf-accent)]">
				{#if isExpanded}
					<FolderOpen size={16} />
				{:else}
					<Folder size={16} />
				{/if}
			</span>
		{:else}
			<span class="w-4 h-4"></span>
			<span class="w-4 h-4 flex items-center justify-center shrink-0 text-[var(--wf-fg-muted)]">
				<svelte:component this={getFileIcon(node.name)} size={16} />
			</span>
		{/if}

		<span class="truncate">{node.name}</span>

		{#if loading}
			<span class="ml-auto text-[var(--wf-fg-muted)] text-xs">加载中...</span>
		{/if}
	</button>

	{#if node.is_dir && isExpanded}
		<div class="file-tree-children">
			{#if hasLoaded && children.length > 0}
				{#each children as child (child.path)}
					<FileTree
						{columnId}
						node={child}
						depth={depth + 1}
						{expandedPaths}
						{selectedPath}
						{onSelect}
					/>
				{/each}
			{:else if hasLoaded && children.length === 0}
				<div
					class="px-2 py-1 text-[12px] text-[var(--wf-fg-muted)]"
					style="padding-left: {(depth + 1) * 16}px"
				>
					空目录
				</div>
			{:else if !hasLoaded}
				<div
					class="px-2 py-1 text-[12px] text-[var(--wf-fg-muted)]"
					style="padding-left: {(depth + 1) * 16}px"
				>
					点击展开
				</div>
			{/if}
		</div>
	{/if}
</div>

<style>
	.file-tree-node button:focus {
		outline: none;
	}

	.file-tree-node button:focus-visible {
		outline: 1px solid var(--wf-accent);
		outline-offset: -1px;
	}
</style>