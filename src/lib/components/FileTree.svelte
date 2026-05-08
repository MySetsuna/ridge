<script lang="ts">
	import { tick, onDestroy } from 'svelte';
import { get } from 'svelte/store';
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
	import { invoke, isTauri } from '@tauri-apps/api/core';
import { writeText, readText } from '@tauri-apps/plugin-clipboard-manager';
	import { showContextMenu } from '$lib/stores/contextMenu';
	import { alertDialog, confirmDialog } from './RidgeDialog.svelte';
	import {
		fileExplorerStore,
		uniqueChildName,
		refreshColumnsCovering,
	} from '$lib/stores/fileExplorer';
	import { fileEditorStore } from '$lib/stores/fileEditor';
	import type { FileNode } from '$lib/stores/project';
	import { searchInFolder } from '$lib/stores/searchState';
	import FileTree from './FileTree.svelte';

	interface Props {
		columnId: string;
		node: FileNode;
		depth?: number;
		expandedPaths?: Set<string>;
		/** Primary (cursor) selected path. Gets a stronger outline ring. */
		selectedPath?: string | null;
		/** Full multi-selection set. Any member gets the highlight tint. */
		selectedPaths?: Set<string>;
		/** Cut clipboard paths — receive a dimmed opacity until paste consumes. */
		cutPaths?: Set<string>;
		onSelect?: (
			path: string,
			isDir: boolean,
			modifiers?: { shift: boolean; ctrl: boolean; meta: boolean }
		) => void;
	}

	let {
		columnId,
		node,
		depth = 0,
		expandedPaths = new Set(),
		selectedPath = null,
		selectedPaths = new Set(),
		cutPaths,
		onSelect,
	}: Props = $props();

	// 资源管理器现在按 depth=1 懒加载：根树只带一层直接子节点，子目录的
	// children 在用户第一次展开时通过 loadChildrenPage 分批拉取（每批
	// DEFAULT_CHILDREN_PAGE_SIZE 条，剩余条数体现在 "加载更多 (剩余 N)" 行）。
	// 当父级 loadTree 已经把 node.children 喂满（兼容历史 depth>1 路径），
	// 我们直接把它当作完整页面接管，避免重复 IPC。
	let childrenPage = $state<FileNode[]>([]);
	let childrenLoadedTotal = $state(0);
	let childrenTotalCount = $state(0);
	let childrenHasMore = $state(false);
	let childrenLoading = $state(false);
	let hasLoaded = $state(false);

	let isExpanded = $derived(expandedPaths.has(node.path));
	/** Primary (cursor) node — single, keyboard-focused. */
	let isPrimary = $derived(selectedPath === node.path);
	/** Any selected — multi-select highlights all members. */
	let isSelected = $derived(selectedPaths.has(node.path) || isPrimary);
	/** Cut state — VSCode dims the row until paste (or Esc) consumes. */
	let isCut = $derived(!!cutPaths && cutPaths.has(node.path));
	/**
	 * Backend marks `is_ignored = true` on entries matched by the cwd's
	 * `.gitignore` chain (or `false` otherwise; `undefined` outside any
	 * git repo). We render those rows with reduced opacity + italic to
	 * match VS Code's gitignored treatment, but they remain fully
	 * interactive — click to open, F2 to rename, Delete to remove.
	 */
	let isIgnored = $derived(node.is_ignored === true);

	/**
	 * Inline edit state. VS Code-style: the node's name swaps to an <input>
	 * when renaming; directory's expanded children get a transient input row
	 * when creating. Enter commits, Escape/Blur cancels.
	 */
	type EditKind = 'rename' | 'create-file' | 'create-folder';
	let editing = $state<EditKind | null>(null);
	let editValue = $state('');
	let editInput: HTMLInputElement | undefined = $state();
	let pendingEditCommit = $state(false);

	// Reset paged state — invoked when a refresh / rename / drop invalidates
	// what we've already loaded. Next expand re-fetches page 0.
	function resetChildrenState(): void {
		childrenPage = [];
		childrenLoadedTotal = 0;
		childrenTotalCount = 0;
		childrenHasMore = false;
		hasLoaded = false;
	}

	// 当父级 loadTree 返回新树后（needsRefresh / 用户刷新 / drop after-effect），
	// node.children 是最新的"完整一页"。直接接管：把它当作 page 0 已加载，
	// hasMore=false（depth=1 下只有 root 自己有 children；其它节点 children
	// 总是 None，走下方懒加载分支）。
	$effect(() => {
		if (node.children) {
			childrenPage = node.children;
			childrenLoadedTotal = node.children.length;
			childrenTotalCount = node.children.length;
			childrenHasMore = false;
			hasLoaded = true;
		}
	});

	// First expand → fetch page 0. Subsequent pages load via the
	// "加载更多" button, not implicit on expand.
	$effect(() => {
		if (isExpanded && node.is_dir && !hasLoaded && !childrenLoading) {
			void loadNextChildrenPage();
		}
	});

	async function loadNextChildrenPage(): Promise<void> {
		if (!node.is_dir) return;
		if (childrenLoading) return;
		if (hasLoaded && !childrenHasMore) return;
		childrenLoading = true;
		try {
			const page = await fileExplorerStore.loadChildrenPage(
				columnId,
				node.path,
				childrenLoadedTotal,
			);
			childrenPage = [...childrenPage, ...page.entries];
			childrenLoadedTotal += page.entries.length;
			childrenTotalCount = page.total;
			childrenHasMore = page.has_more;
			hasLoaded = true;
		} catch (e) {
			console.error('Failed to load children page:', e);
		} finally {
			childrenLoading = false;
		}
	}

	// ─── Drag & drop (move by default, Ctrl = copy) ────────────────────────
	// MIME used for the dragged payload: newline-separated absolute paths.
	// Keeps the protocol browser-native so drops into external apps still
	// convey something useful (many shells accept text lists of paths).
	const DND_TYPE = 'application/x-ridge-explorer-paths';
	/**
	 * Hover-to-expand latency: matches VS Code / macOS Finder "spring-loaded
	 * folders" behaviour — pause over a collapsed dir during drag ~800ms to
	 * automatically open it and drill in.
	 */
	const HOVER_EXPAND_MS = 800;
	let isDragTarget = $state(false);
	let hoverExpandTimer: ReturnType<typeof setTimeout> | null = null;

	function clearHoverExpandTimer(): void {
		if (hoverExpandTimer !== null) {
			clearTimeout(hoverExpandTimer);
			hoverExpandTimer = null;
		}
	}
	onDestroy(clearHoverExpandTimer);

	function onNodeDragStart(e: DragEvent): void {
		if (editing) return;
		// If the current node is part of a multi-selection, drag the whole set.
		// Otherwise drag only this one path.
		const payloadPaths =
			selectedPaths.has(node.path) && selectedPaths.size > 1
				? Array.from(selectedPaths)
				: [node.path];
		if (!e.dataTransfer) return;
		e.dataTransfer.effectAllowed = 'copyMove';
		e.dataTransfer.setData(DND_TYPE, payloadPaths.join('\n'));
		// Fallback: plain text of the paths so drops into terminals / editors
		// still get something human-readable.
		e.dataTransfer.setData('text/plain', payloadPaths.join('\n'));
	}

	function onNodeDragOver(e: DragEvent): void {
		if (!node.is_dir) return;
		const types = e.dataTransfer?.types;
		if (!types || !Array.from(types).includes(DND_TYPE)) return;
		// Only accept drops that would actually move/copy across paths. Prevent
		// default to signal "yes, this is a valid drop zone".
		e.preventDefault();
		if (e.dataTransfer) {
			e.dataTransfer.dropEffect = e.ctrlKey || e.metaKey ? 'copy' : 'move';
		}
		if (!isDragTarget) {
			isDragTarget = true;
			// Only arm the auto-expand timer once per hover, when entering the
			// row. dragover fires repeatedly; we don't want to reset the clock
			// on every pixel of movement.
			if (node.is_dir && !isExpanded) {
				clearHoverExpandTimer();
				hoverExpandTimer = setTimeout(() => {
					// Sanity check: still being hovered with drag active.
					if (isDragTarget) {
						fileExplorerStore.toggleExpanded(columnId, node.path);
					}
					hoverExpandTimer = null;
				}, HOVER_EXPAND_MS);
			}
		}
	}

	function onNodeDragLeave(): void {
		isDragTarget = false;
		clearHoverExpandTimer();
	}

	async function onNodeDrop(e: DragEvent): Promise<void> {
		isDragTarget = false;
		clearHoverExpandTimer();
		if (!node.is_dir) return;
		const raw = e.dataTransfer?.getData(DND_TYPE);
		if (!raw) return;
		e.preventDefault();
		e.stopPropagation();

	// Get column cwd for relative path
	const storeState = get(fileExplorerStore);
	const column = storeState.columns.find((c) => c.id === columnId);
	const cwd = column?.cwd || '';

	const getRelativePath = (absPath: string): string => {
		const normalizedCwd = cwd.replace(/\\/g, '/').replace(/\/+$/, '');
		const normalizedPath = absPath.replace(/\\/g, '/');
		if (normalizedPath.startsWith(normalizedCwd + '/')) {
			return normalizedPath.slice(normalizedCwd.length + 1);
		}
		return normalizedPath;
	};

	const copyToClipboard = async (text: string) => {
		try { await writeText(text); } catch (err) { console.error('Copy failed:', err); }
	};
		const paths = raw.split('\n').filter(Boolean);
		if (paths.length === 0) return;
		// Refuse self-drop / drop of an ancestor onto a descendant.
		for (const p of paths) {
			if (p === node.path || node.path.startsWith(p + '/') || node.path.startsWith(p + '\\')) {
				await alertDialog({ title: '拖放失败', message: '不能把路径拖到它自己或其子目录中', danger: true });

				return;
			}
		}
		if (!isTauri()) return;
		const copy = e.ctrlKey || e.metaKey;
		const cmd = copy ? 'copy_path' : 'move_path';
		// Names already in the target dir — auto-rename on conflict so we
		// never silently clobber. Shared helper with paste path.
		// Drag-drop is rare and needs the FULL child list (not just the
		// pages the user has expanded into). Use the legacy
		// paginate-then-concat wrapper so cross-page conflicts (e.g.
		// drop "foo.ts" into a folder whose existing "foo.ts" is on
		// page 4) are still caught.
		const fullChildren = await fileExplorerStore.loadChildren(columnId, node.path);
		const existing = new Set<string>(fullChildren.map((c) => c.path));
		const sep = node.path.includes('\\') && !node.path.includes('/') ? '\\' : '/';
		const cleanTarget = node.path.replace(/[\\/]+$/, '');
		const errors: string[] = [];
		for (const from of paths) {
			const leaf = from.split(/[\\/]/).pop() || 'untitled';
			const unique = uniqueChildName(node.path, leaf, existing);
			const to = `${cleanTarget}${sep}${unique}`;
			existing.add(to);
			try {
				await invoke(cmd, { from, to });
			} catch (err) {
				errors.push(`${from}: ${err}`);
			}
		}
		// Reload target + any column caching the target dir (covers "two
		// workspaces at same cwd" scenarios). For moves, also refresh the
		// source parents so the row disappears there.
		resetChildrenState();
		await refreshColumnsCovering(node.path);
		if (!copy) {
			const sourceDirs = new Set<string>(
				paths.map((p) => p.replace(/[\\/][^\\/]+[\\/]*$/, '') || p)
			);
			for (const d of sourceDirs) await refreshColumnsCovering(d);
		}
		if (errors.length > 0) {
			await alertDialog({ title: '拖放失败', message: `${errors.length} 项拖放失败:\n${errors.join('\n')}`, danger: true });
		}
	}

	function activateNode(modifiers: { shift: boolean; ctrl: boolean; meta: boolean }) {
		if (editing) return;
		// Dir toggle only on plain click (no modifier) so Shift/Ctrl selections
		// don't accidentally collapse/expand dirs while the user builds a range.
		if (node.is_dir && !modifiers.shift && !modifiers.ctrl && !modifiers.meta) {
			fileExplorerStore.toggleExpanded(columnId, node.path);
		}
		if (onSelect) {
			onSelect(node.path, node.is_dir, modifiers);
		}
		// Explorer.handleFileSelect now owns setSelection (Shift/Ctrl aware).
		// If no handler is plugged we fall back to single-select.
		if (!onSelect) {
			fileExplorerStore.setSelectedPath(columnId, node.path);
		}
	}

	function handleClick(e: MouseEvent) {
		activateNode({ shift: e.shiftKey, ctrl: e.ctrlKey, meta: e.metaKey });
	}

	/**
	 * Keyboard contract (subset of VS Code tree list):
	 *   Enter / Space → activate (open file / toggle dir)
	 *   ArrowRight    → expand directory
	 *   ArrowLeft     → collapse directory
	 *   F2            → rename
	 *   Delete        → delete
	 */
	function handleKeydown(e: KeyboardEvent) {
		if (editing || e.isComposing) return;
		switch (e.key) {
			case 'Enter':
			case ' ':
				e.preventDefault();
				// Keyboard activate forwards modifier state from the keydown event
				// so Shift+Enter / Ctrl+Enter can mirror Shift-click / Ctrl-click
				// on the currently-focused row.
				activateNode({ shift: e.shiftKey, ctrl: e.ctrlKey, meta: e.metaKey });
				return;
			case 'ArrowRight':
				if (node.is_dir && !isExpanded) {
					e.preventDefault();
					fileExplorerStore.toggleExpanded(columnId, node.path);
				}
				return;
			case 'ArrowLeft':
				if (node.is_dir && isExpanded) {
					e.preventDefault();
					fileExplorerStore.toggleExpanded(columnId, node.path);
				}
				return;
			case 'F2':
				e.preventDefault();
				beginRename();
				return;
			case 'Delete':
				e.preventDefault();
				void deleteItem();
				return;
		}
	}

	function handleContextMenu(e: MouseEvent) {
		e.preventDefault();
		e.stopPropagation();

	// Get column cwd for relative path
	const storeState = get(fileExplorerStore);
	const column = storeState.columns.find((c) => c.id === columnId);
	const cwd = column?.cwd || '';

	const getRelativePath = (absPath: string): string => {
		const normalizedCwd = cwd.replace(/\\/g, '/').replace(/\/+$/, '');
		const normalizedPath = absPath.replace(/\\/g, '/');
		if (normalizedPath.startsWith(normalizedCwd + '/')) {
			return normalizedPath.slice(normalizedCwd.length + 1);
		}
		return normalizedPath;
	};

	const copyToClipboard = async (text: string) => {
		try { await writeText(text); } catch (err) { console.error('Copy failed:', err); }
	};

		const items = node.is_dir
			? [
					{ id: 'new-file', label: '新建文件', action: () => beginCreate('file') },
					{ id: 'new-folder', label: '新建文件夹', action: () => beginCreate('folder') },
					{ id: 'divider1', divider: true },
			{ id: 'copy', label: '复制', action: () => copyToClipboard(node.path) },			{ id: 'copy-rel', label: '复制相对路径', action: () => copyToClipboard(getRelativePath(node.path)) },
					{ id: 'reveal', label: '在文件管理器中显示', action: () => void revealInExplorer() },
					{ id: 'search-in-folder', label: '在文件夹中搜索', action: () => searchInFolder(node.path) },
					{ id: 'divider2', divider: true },
					{ id: 'rename', label: '重命名', action: () => beginRename() },
					{ id: 'delete', label: '删除', action: () => void deleteItem() },
				]
			: [
					{ id: 'open', label: '打开', action: () => void openFile() },
			{ id: 'copy', label: '复制', action: () => copyToClipboard(node.path) },			{ id: 'copy-rel', label: '复制相对路径', action: () => copyToClipboard(getRelativePath(node.path)) },
					{ id: 'reveal', label: '在文件管理器中显示', action: () => void revealInExplorer() },
					{ id: 'divider', divider: true },
					{ id: 'rename', label: '重命名', action: () => beginRename() },
					{ id: 'delete', label: '删除', action: () => void deleteItem() },
				];

		showContextMenu(e.clientX, e.clientY, items);
	}

	/**
	 * Normalise a base directory + a new leaf name into an absolute path using
	 * the base's own separator style. Matches the logic in MarkdownPreview so
	 * Windows `C:\` paths stay consistent through the create pipeline.
	 */
	function joinChild(baseDir: string, child: string): string {
		const sep = baseDir.includes('\\') && !baseDir.includes('/') ? '\\' : '/';
		const cleanBase = baseDir.replace(/[\\/]+$/, '');
		return `${cleanBase}${sep}${child}`;
	}

	function parentDir(path: string): string {
		return path.replace(/[\\/][^\\/]+[\\/]*$/, '') || path;
	}

	async function refreshColumnTree(): Promise<void> {
		await fileExplorerStore.loadTree(columnId);
	}

	async function focusAndSelectEditInput(kind: EditKind): Promise<void> {
		await tick();
		editInput?.focus();
		if (kind === 'rename') {
			// Select everything, or the basename portion for files with extensions.
			const dotIdx = node.name.lastIndexOf('.');
			if (!node.is_dir && dotIdx > 0) {
				editInput?.setSelectionRange(0, dotIdx);
			} else {
				editInput?.select();
			}
		}
	}

	function beginRename(): void {
		editing = 'rename';
		editValue = node.name;
		void focusAndSelectEditInput('rename');
	}

	function beginCreate(kind: 'file' | 'folder'): void {
		if (!node.is_dir) return;
		if (!isExpanded) fileExplorerStore.toggleExpanded(columnId, node.path);
		editing = kind === 'file' ? 'create-file' : 'create-folder';
		editValue = '';
		void focusAndSelectEditInput(editing);
	}

	function cancelEdit(): void {
		editing = null;
		editValue = '';
	}

	async function commitEdit(): Promise<void> {
		if (!editing || pendingEditCommit) return;
		const val = editValue.trim();
		const currentEditing = editing;
		if (!val) {
			cancelEdit();
			return;
		}
		pendingEditCommit = true;
		try {
			if (currentEditing === 'rename') {
				if (val === node.name) {
					cancelEdit();
					return;
				}
				const target = joinChild(parentDir(node.path), val);
				if (!isTauri()) {
					cancelEdit();
					return;
				}
				try {
					await invoke('rename_path', { from: node.path, to: target });
					await refreshColumnTree();
				} catch (e) {
					await alertDialog({ title: '重命名失败', message: String(e), danger: true });
					return;
				}
			} else if (currentEditing === 'create-file' || currentEditing === 'create-folder') {
				const isFile = currentEditing === 'create-file';
				const target = joinChild(node.path, val);
				if (!isTauri()) {
					cancelEdit();
					return;
				}
				try {
					await invoke(isFile ? 'create_file' : 'create_directory', { path: target });
					resetChildrenState();
					await refreshColumnTree();
					if (isFile) await fileEditorStore.openFile(target);
				} catch (e) {
					await alertDialog({ title: '创建失败', message: `${isFile ? '创建文件' : '创建目录'}失败: ${e}`, danger: true });
					return;
				}
			}
			cancelEdit();
		} finally {
			pendingEditCommit = false;
		}
	}

	function onEditKeydown(e: KeyboardEvent): void {
		if (e.isComposing) return;
		if (e.key === 'Enter') {
			e.preventDefault();
			void commitEdit();
		} else if (e.key === 'Escape') {
			e.preventDefault();
			cancelEdit();
		}
	}

	function onEditBlur(): void {
		// Blur triggers commit unless the user just hit Escape (in which case
		// `editing` is already null and commitEdit is a no-op).
		if (editing && !pendingEditCommit) void commitEdit();
	}

	async function revealInExplorer(): Promise<void> {
		if (!isTauri()) return;
		try {
			await invoke('reveal_in_file_manager', { path: node.path });
		} catch (e) {
			await alertDialog({ title: '操作失败', message: `打开文件管理器失败: ${e}`, danger: true });
		}
	}

	async function openFile(): Promise<void> {
		if (node.is_dir) return;
		await fileEditorStore.openFile(node.path);
	}

	async function deleteItem(): Promise<void> {
		if (!isTauri()) return;
		// If this node is part of a multi-selection, act on the whole set.
		// Otherwise fall back to single-node delete (legacy behaviour).
		const multi = selectedPaths.has(node.path) && selectedPaths.size > 1;
		const targets = multi ? Array.from(selectedPaths) : [node.path];
		const label =
			targets.length > 1
				? `${targets.length} 项（含 "${node.name}"）`
				: `"${node.name}"`;
		const confirmed = await confirmDialog({
			title: '删除文件',
			message: `确认删除 ${label}？此操作不可撤销。`,
			okLabel: '删除',
			danger: true,
		});
		if (!confirmed) return;
		// Delete sequentially; continue on individual failures so one "already
		// gone" target doesn't abort the rest. Collect errors and report a
		// summary at the end.
		const errors: string[] = [];
		for (const p of targets) {
			try {
				await invoke('delete_path', { path: p });
			} catch (e) {
				errors.push(`${p}: ${e}`);
			}
		}
		await refreshColumnTree();
		if (errors.length > 0) {
			await alertDialog({
				title: '部分删除失败',
				message: `${errors.length} 项删除失败:\n${errors.join('\n')}`,
				danger: true,
			});
		}
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

	const FileIcon = $derived(getFileIcon(node.name));

	const PendingCreateIcon = $derived(editing === 'create-file' ? File : Folder);
</script>

<div class="file-tree-node" style="padding-left: {depth * 8}px">
	<button
		type="button"
		class="flex w-full items-center gap-1.5 px-2 py-1 text-left text-[13px] transition-colors hover:bg-[var(--rg-accent)]/10 {isSelected
			? 'bg-[var(--rg-accent)]/20 text-[var(--rg-accent)]'
			: 'text-[var(--rg-fg)]'} {isPrimary ? 'ring-1 ring-inset ring-[var(--rg-accent)]/60' : ''} {isCut
			? 'opacity-50'
			: ''} {isDragTarget
			? 'bg-[var(--rg-accent)]/30 ring-2 ring-inset ring-[var(--rg-accent)]'
			: ''} {isIgnored ? 'rg-tree-ignored' : ''}"
		data-rg-tree-path={node.path}
		data-rg-tree-column={columnId}
		data-rg-ignored={isIgnored ? 'true' : null}
		draggable="true"
		ondragstart={onNodeDragStart}
		ondragover={onNodeDragOver}
		ondragleave={onNodeDragLeave}
		ondrop={(e) => void onNodeDrop(e)}
		onclick={handleClick}
		onkeydown={handleKeydown}
		oncontextmenu={handleContextMenu}
	>
		{#if node.is_dir}
			<span class="w-4 h-4 flex items-center justify-center shrink-0 text-[var(--rg-fg-muted)]">
				{#if isExpanded}
					<ChevronDown size={14} />
				{:else}
					<ChevronRight size={14} />
				{/if}
			</span>
			<span class="w-4 h-4 flex items-center justify-center shrink-0 text-[var(--rg-accent)]">
				{#if isExpanded}
					<FolderOpen size={16} />
				{:else}
					<Folder size={16} />
				{/if}
			</span>
		{:else}
			<span class="w-4 h-4"></span>
			<span class="w-4 h-4 flex items-center justify-center shrink-0 text-[var(--rg-fg-muted)]">
				<FileIcon size={16} />
			</span>
		{/if}

		{#if editing === 'rename'}
			<!-- 内联重命名 input：阻止 button 的 click/contextmenu/keydown，避免触发选中/展开。 -->
			<input
				type="text"
				bind:this={editInput}
				bind:value={editValue}
				class="flex-1 min-w-0 bg-[var(--rg-bg)] border border-[var(--rg-accent)]/60 outline-none rounded px-1 text-[13px] text-[var(--rg-fg)]"
				onkeydown={onEditKeydown}
				onblur={onEditBlur}
				onclick={(e) => e.stopPropagation()}
				oncontextmenu={(e) => e.stopPropagation()}
			/>
		{:else}
			<span class="truncate">{node.name}</span>
		{/if}
	</button>

	{#if node.is_dir && isExpanded}
		<div class="file-tree-children">
			{#if editing === 'create-file' || editing === 'create-folder'}
				<!-- 新建条目占位行：深度 = depth + 1；图标按 kind 切换。 -->
				<div class="file-tree-node" style="padding-left: {(depth + 1) * 8}px">
					<div
						class="flex w-full items-center gap-1.5 px-2 py-1 text-[13px] bg-[var(--rg-accent)]/10"
					>
						<span class="w-4 h-4"></span>
						<span class="w-4 h-4 flex items-center justify-center shrink-0 text-[var(--rg-accent)]">
							<PendingCreateIcon size={16} />
						</span>
						<input
							type="text"
							bind:this={editInput}
							bind:value={editValue}
							placeholder={editing === 'create-file' ? '新文件名' : '新文件夹名'}
							class="flex-1 min-w-0 bg-[var(--rg-bg)] border border-[var(--rg-accent)]/60 outline-none rounded px-1 text-[13px] text-[var(--rg-fg)]"
							onkeydown={onEditKeydown}
							onblur={onEditBlur}
						/>
					</div>
				</div>
			{/if}
			{#if hasLoaded && childrenPage.length > 0}
				{#each childrenPage as child (child.path)}
					<FileTree
						{columnId}
						node={child}
						depth={depth + 1}
						{expandedPaths}
						{selectedPath}
						{selectedPaths}
						{cutPaths}
						{onSelect}
					/>
				{/each}
			{:else if hasLoaded && childrenPage.length === 0 && !editing}
				<div
					class="px-2 py-1 text-[12px] text-[var(--rg-fg-muted)]"
					style="padding-left: {(depth + 1) * 8}px"
				>
					空目录
				</div>
			{/if}
			{#if childrenHasMore}
				<!--
					Paged "load more" row. Visible only when the backend reported
					additional entries beyond what we've fetched. Disabled while
					a fetch is in flight so a double-click doesn't double-page.
				-->
				<div class="file-tree-node" style="padding-left: {(depth + 1) * 8}px">
					<button
						type="button"
						class="flex w-full items-center gap-1.5 px-2 py-1 text-left text-[12px] text-[var(--rg-fg-muted)] hover:bg-[var(--rg-accent)]/10 disabled:opacity-50"
						onclick={() => void loadNextChildrenPage()}
						disabled={childrenLoading}
					>
						<span class="w-4 h-4"></span>
						<span class="w-4 h-4"></span>
						{#if childrenLoading}
							加载中…
						{:else}
							加载更多 (剩余 {Math.max(0, childrenTotalCount - childrenLoadedTotal)})
						{/if}
					</button>
				</div>
			{/if}
			<!-- 展开过程中不再显示"点击展开"/"加载中"文本，避免子列表在几 ms 内抖动出现。 -->
		</div>
	{/if}
</div>

<style>
	.file-tree-node button:focus {
		outline: none;
	}

	.file-tree-node button:focus-visible {
		outline: 1px solid var(--rg-accent);
		outline-offset: -1px;
	}

	/*
	 * Gitignored row treatment — VS Code parity. Italic + 50% opacity
	 * + muted foreground; selection highlight still wins because the
	 * selected-row class is applied alongside `rg-tree-ignored` and
	 * raises foreground/background to the accent palette.
	 */
	.file-tree-node :global(.rg-tree-ignored) {
		opacity: 0.5;
		font-style: italic;
		color: var(--rg-fg-muted);
	}
</style>
