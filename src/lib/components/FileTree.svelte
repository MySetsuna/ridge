<script module lang="ts">
	import { writable } from 'svelte/store';
	// 当前拖拽落点文件夹的绝对路径，所有 FileTree 实例共享，用于落点高亮。
	const dragOverPath = writable<string | null>(null);
</script>

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
import { writeText } from '@tauri-apps/plugin-clipboard-manager';
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
	import { activePaneId } from '$lib/stores/paneTree';
	import { t, tr } from '$lib/i18n';
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
		/**
		 * 父级 loadTree 完成时由 ExplorerColumn 单调递增的计数器。每次 bump 触发
		 * 本组件 reset 分页状态 + 若已展开则重新拉首页 —— 因为 depth=1 树仅刷新
		 * cwd 直系子节点，孙子目录的分页内容完全活在本组件的 component-local
		 * state 里，store 那次 update 看不到也碰不到，必须靠这个 nonce 单向通知。
		 */
		refreshNonce?: number;
		/**
		 * 任一祖先目录命中 .gitignore 时为 true。后端 `is_ignored` 是逐项独立计算
		 * 的，但 ignored 视觉应该向下继承到所有子孙节点（VS Code 行为：
		 * `node_modules/` 灰，里面所有内容也灰）。父组件递归把自己的 `isIgnored`
		 * 通过这个 prop 下传，子组件 OR 自己的 `node.is_ignored` 即可。
		 */
		inheritedIgnored?: boolean;
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
		refreshNonce = 0,
		inheritedIgnored = false,
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
	let childrenError = $state<string | null>(null);

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
	 * git repo). We render those rows with reduced opacity / muted color
	 * — no italic — to match VS Code's gitignored treatment. Visual state
	 * cascades to descendants via `inheritedIgnored`, so once a folder is
	 * ignored the entire subtree under it stays grayed regardless of what
	 * the per-entry backend flag reports for individual children. Rows
	 * remain fully interactive — click to open, F2 to rename, Delete to
	 * remove.
	 */
	let isIgnored = $derived(inheritedIgnored || node.is_ignored === true);

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
		childrenError = null;
	}

	// 当父级 loadTree 返回新树后（needsRefresh / 用户刷新 / drop after-effect），
	// node.children 是最新的"完整一页"。直接接管：把它当作 page 0 已加载，
	// hasMore=false（depth=1 下只有 root 自己有 children；其它节点 children
	// 总是 None，走下方懒加载分支）。
	// 使用 Array.isArray 且长度 > 0 判断：Rust 端空目录返回 Some([]) 而非 None，
	// 空数组 [] 在 JS 中 truthy，若仅用 `if (node.children)` 会错误地锁定
	// hasLoaded=true 阻止懒加载。
	$effect(() => {
		if (Array.isArray(node.children) && node.children.length > 0) {
			childrenPage = node.children;
			childrenLoadedTotal = node.children.length;
			childrenTotalCount = node.children.length;
			childrenHasMore = false;
			hasLoaded = true;
		}
	});

	// 列级 refreshNonce bump 处理：
	//   • depth=1 直系子（node.children 有值）已经被上面的 $effect 接管成「最新
	//     一页」，无需额外动作；
	//   • 孙子级目录（node.children 为 None）的分页结果只在本组件 state 里。
	//     bump 后：
	//       - 若当前已展开且有旧数据：保留旧 childrenPage 避免闪白，后台拉新页后原子替换
	//       - 否则：reset + 拉首页
	let prevRefreshNonce = $state<number | undefined>(undefined);
	$effect(() => {
		const nonce = refreshNonce;
		if (prevRefreshNonce !== undefined && prevRefreshNonce !== nonce && !Array.isArray(node.children)) {
			if (isExpanded && node.is_dir && childrenPage.length > 0) {
				childrenLoading = true;
				void loadNextChildrenPage(true);
			} else {
				resetChildrenState();
				if (isExpanded && node.is_dir) {
					void loadNextChildrenPage();
				}
			}
		}
		prevRefreshNonce = nonce;
	});

	// First expand → fetch page 0. Subsequent pages load via the
	// "加载更多" button, not implicit on expand.
	// 错误时 hasLoaded=true，不会重复触发；用户可点击错误文本重试。
	$effect(() => {
		if (isExpanded && node.is_dir && !hasLoaded && !childrenLoading) {
			void loadNextChildrenPage();
		}
	});

	async function loadNextChildrenPage(replace = false): Promise<void> {
		if (!node.is_dir) return;
		if (!replace && childrenLoading) return;
		if (!replace && hasLoaded && !childrenHasMore) return;
		if (childrenError) childrenError = null;
		childrenLoading = true;
		try {
			const page = await fileExplorerStore.loadChildrenPage(
				columnId,
				node.path,
				replace ? 0 : childrenLoadedTotal,
			);
			if (replace) {
				childrenPage = page.entries;
				childrenLoadedTotal = page.entries.length;
			} else {
				childrenPage = [...childrenPage, ...page.entries];
				childrenLoadedTotal += page.entries.length;
			}
			childrenTotalCount = page.total;
			childrenHasMore = page.has_more;
			childrenError = null;
			hasLoaded = true;
		} catch (e) {
			console.error('Failed to load children page:', e);
			const msg = String(e);
			if (/not exist|No such file/i.test(msg)) {
				// Directory was deleted while expanded — clean collapse,
				// no error message shown.
				fileExplorerStore.collapseOnLoadError(columnId, node.path);
			} else {
				childrenError = tr('explorer.loadFailed');
			}
			hasLoaded = true;
		} finally {
			childrenLoading = false;
		}
	}

	// ─── 拖拽（指针事件实现）─────────────────────────────────────────────────
	// WebView2 下 Tauri 原生 dragDrop 会吞掉 webview 内 HTML5 drop（见记忆
	// project_webview2_dnd），故文件树拖拽改用指针事件：pointerdown 起拖、
	// setPointerCapture 让跨元素（树↔树、树→终端）的 pointermove/up 都回到源按钮，
	// elementFromPoint 命中落点。落文件夹 = 移动（Ctrl/⌘ = 复制），落终端 = 粘路径文本。
	const DRAG_THRESHOLD_PX = 4;
	// Hover-to-expand：拖拽悬停折叠目录 ~800ms 自动展开（spring-loaded folders）。
	const HOVER_EXPAND_MS = 800;

	// 本节点是否为当前拖拽落点（高亮）。dragOverPath 是模块级共享 store（见顶部
	// <script module>），跨 FileTree 实例广播落点路径。
	let isDragTarget = $derived($dragOverPath === node.path && node.is_dir);

	let dragPointerId: number | null = null;
	let dragStartX = 0;
	let dragStartY = 0;
	let dragging = false;
	// 刚发生过拖拽 —— 让紧随 pointerup 的 click 不再触发选中/展开。
	let didDrag = false;
	let dragPaths: string[] = [];
	let dropFolderPath: string | null = null;
	let dropPaneId: string | null = null;
	let ghostEl: HTMLDivElement | null = null;
	let hoverExpandTimer: ReturnType<typeof setTimeout> | null = null;
	let hoverExpandPath: string | null = null;

	function clearHoverExpandTimer(): void {
		if (hoverExpandTimer !== null) {
			clearTimeout(hoverExpandTimer);
			hoverExpandTimer = null;
		}
		hoverExpandPath = null;
	}

	function removeGhost(): void {
		if (ghostEl) { ghostEl.remove(); ghostEl = null; }
	}

	function createGhost(count: number, label: string): void {
		const g = document.createElement('div');
		g.textContent = count > 1 ? tr('explorer.dragGhostCount', { count }) : label;
		g.style.cssText =
			'position:fixed;z-index:9999;pointer-events:none;left:0;top:0;padding:2px 8px;' +
			'border-radius:6px;font-size:12px;line-height:1.4;white-space:nowrap;color:var(--rg-fg);' +
			'background:var(--rg-surface-2);border:1px solid var(--rg-accent);' +
			'box-shadow:0 4px 12px rgba(0,0,0,.3);opacity:.95;';
		document.body.appendChild(g);
		ghostEl = g;
	}

	function moveGhost(x: number, y: number): void {
		if (ghostEl) ghostEl.style.transform = `translate(${x + 12}px, ${y + 8}px)`;
	}

	// 落点文件夹合法性：排除拖到自身、或把祖先拖进自己的后代。
	function isValidDropFolder(target: string): boolean {
		for (const p of dragPaths) {
			if (target === p) return false;
			if (target.startsWith(p + '/') || target.startsWith(p + '\\')) return false;
		}
		return true;
	}

	function armHoverExpand(path: string): void {
		if (hoverExpandPath === path) return;
		clearHoverExpandTimer();
		hoverExpandPath = path;
		if (!expandedPaths.has(path)) {
			hoverExpandTimer = setTimeout(() => {
				if (dragging && hoverExpandPath === path) {
					fileExplorerStore.toggleExpanded(columnId, path);
				}
				hoverExpandTimer = null;
			}, HOVER_EXPAND_MS);
		}
	}

	function onNodePointerDown(e: PointerEvent): void {
		if (e.button !== 0 || editing) return;
		dragPointerId = e.pointerId;
		dragStartX = e.clientX;
		dragStartY = e.clientY;
		dragging = false;
		didDrag = false;
		try {
			(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
		} catch {
			/* capture 可能失败，忽略 */
		}
	}

	function beginDrag(): void {
		dragging = true;
		// 多选拖拽：本节点在多选集合内则拖整个集合，否则只拖自己。
		dragPaths =
			selectedPaths.has(node.path) && selectedPaths.size > 1
				? Array.from(selectedPaths)
				: [node.path];
		createGhost(dragPaths.length, node.name);
	}

	function onNodePointerMove(e: PointerEvent): void {
		if (dragPointerId === null) return;
		if (!dragging) {
			if (Math.hypot(e.clientX - dragStartX, e.clientY - dragStartY) < DRAG_THRESHOLD_PX) return;
			beginDrag();
		}
		moveGhost(e.clientX, e.clientY);
		// ghost 设了 pointer-events:none，不会挡住 elementFromPoint。
		const el = document.elementFromPoint(e.clientX, e.clientY) as HTMLElement | null;
		const treeBtn = el?.closest('[data-rg-tree-path]') as HTMLElement | null;
		const paneEl = el?.closest('[data-rg-pane-id]') as HTMLElement | null;
		dropFolderPath = null;
		dropPaneId = null;
		if (treeBtn && treeBtn.getAttribute('data-rg-tree-dir') === 'true') {
			const p = treeBtn.getAttribute('data-rg-tree-path');
			const col = treeBtn.getAttribute('data-rg-tree-column');
			if (p && col === columnId && isValidDropFolder(p)) {
				dropFolderPath = p;
				armHoverExpand(p);
			} else {
				clearHoverExpandTimer();
			}
		} else if (paneEl) {
			dropPaneId = paneEl.getAttribute('data-rg-pane-id');
			clearHoverExpandTimer();
		} else {
			clearHoverExpandTimer();
		}
		dragOverPath.set(dropFolderPath);
	}

	function cleanupDrag(e: PointerEvent): void {
		const el = e.currentTarget as HTMLElement | null;
		if (dragPointerId !== null && el?.hasPointerCapture?.(dragPointerId)) {
			try {
				el.releasePointerCapture(dragPointerId);
			} catch {
				/* */
			}
		}
		dragPointerId = null;
		dragging = false;
		removeGhost();
		clearHoverExpandTimer();
		dragOverPath.set(null);
		dropFolderPath = null;
		dropPaneId = null;
		dragPaths = [];
	}

	async function onNodePointerUp(e: PointerEvent): Promise<void> {
		const wasDragging = dragging;
		const folder = dropFolderPath;
		const pane = dropPaneId;
		const copy = e.ctrlKey || e.metaKey;
		const paths = dragPaths.slice();
		cleanupDrag(e);
		if (!wasDragging) return; // 普通点击：交给 onclick 处理选中/展开。
		didDrag = true;
		if (folder) {
			await performFolderDrop(folder, paths, copy);
		} else if (pane) {
			pasteToTerminal(pane, paths);
		}
	}

	function onNodePointerCancel(e: PointerEvent): void {
		didDrag = false;
		cleanupDrag(e);
	}

	// 落到文件夹：移动（Ctrl/⌘ = 复制）。沿用既有自冲突重命名 + 跨列刷新逻辑。
	async function performFolderDrop(targetFolder: string, paths: string[], copy: boolean): Promise<void> {
		if (!isTauri() || paths.length === 0) return;
		const cmd = copy ? 'copy_path' : 'move_path';
		// 需要目标目录的「完整」子列表（不止已展开分页）以便跨页冲突也能命中。
		const fullChildren = await fileExplorerStore.loadChildren(columnId, targetFolder);
		const existing = new Set<string>(fullChildren.map((c) => c.path));
		const sep = targetFolder.includes('\\') && !targetFolder.includes('/') ? '\\' : '/';
		const cleanTarget = targetFolder.replace(/[\\/]+$/, '');
		const errors: string[] = [];
		for (const from of paths) {
			const leaf = from.split(/[\\/]/).pop() || 'untitled';
			const unique = uniqueChildName(targetFolder, leaf, existing);
			const to = `${cleanTarget}${sep}${unique}`;
			existing.add(to);
			try {
				await invoke(cmd, { from, to });
			} catch (err) {
				errors.push(`${from}: ${err}`);
			}
		}
		resetChildrenState();
		await refreshColumnsCovering(targetFolder);
		if (!copy) {
			const sourceDirs = new Set<string>(
				paths.map((p) => p.replace(/[\\/][^\\/]+[\\/]*$/, '') || p)
			);
			for (const d of sourceDirs) await refreshColumnsCovering(d);
		}
		if (errors.length > 0) {
			await alertDialog({ title: tr('explorer.dndFailed'), message: tr('explorer.dndFailedMessage', { count: errors.length, details: errors.join('\n') }), danger: true });
		}
	}

	// 落到终端 pane：把路径作为文本写入 PTY（带空格的路径加引号 + 末尾空格），
	// 与「从系统资源管理器拖文件进终端」(+page.svelte insertDroppedPaths) 行为一致。
	function pasteToTerminal(paneId: string, paths: string[]): void {
		if (!isTauri() || paths.length === 0) return;
		const quote = (s: string) => (/\s/.test(s) ? `"${s.replace(/"/g, '\\"')}"` : s);
		const text = paths.map(quote).join(' ') + ' ';
		activePaneId.set(paneId);
		void invoke('write_to_pty', { paneId, data: text }).catch((err) => {
			console.error('write_to_pty (tree-drag) failed', err);
		});
	}

	onDestroy(() => {
		clearHoverExpandTimer();
		removeGhost();
		if (dragging) dragOverPath.set(null);
	});

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
		// 刚拖拽完的 pointerup 会带出一个 click —— 吞掉它，避免误触选中/展开。
		if (didDrag) { didDrag = false; return; }
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

		// Capture the node's path at the time the menu is shown (avoid reactive
		// closure issues where `node` changes between show and action execution).
		const pathAtMenu = node.path;
		const isDirAtMenu = node.is_dir;

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

		const items = isDirAtMenu
			? [
					{ id: 'new-file', label: tr('explorer.ctxNewFile'), action: () => beginCreate('file') },
					{ id: 'new-folder', label: tr('explorer.ctxNewFolder'), action: () => beginCreate('folder') },
					{ id: 'divider1', divider: true },
			{ id: 'copy', label: tr('explorer.ctxCopy'), action: () => copyToClipboard(node.path) },			{ id: 'copy-rel', label: tr('explorer.ctxCopyRelative'), action: () => copyToClipboard(getRelativePath(node.path)) },
					{ id: 'reveal', label: tr('explorer.ctxReveal'), action: () => void revealInExplorer() },
					{ id: 'search-in-folder', label: tr('explorer.ctxSearchInFolder'), action: () => searchInFolder(node.path) },
					{ id: 'divider2', divider: true },
					{ id: 'rename', label: tr('explorer.ctxRename'), action: () => beginRename() },
					{ id: 'delete', label: tr('explorer.ctxDelete'), action: () => void deleteItem() },
				]
			: [
					{ id: 'open', label: tr('explorer.ctxOpen'), action: () => void fileEditorStore.openFile(pathAtMenu) },
			{ id: 'copy', label: tr('explorer.ctxCopy'), action: () => copyToClipboard(node.path) },			{ id: 'copy-rel', label: tr('explorer.ctxCopyRelative'), action: () => copyToClipboard(getRelativePath(node.path)) },
					{ id: 'reveal', label: tr('explorer.ctxReveal'), action: () => void revealInExplorer() },
					{ id: 'divider', divider: true },
					{ id: 'rename', label: tr('explorer.ctxRename'), action: () => beginRename() },
					{ id: 'delete', label: tr('explorer.ctxDelete'), action: () => void deleteItem() },
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
					await alertDialog({ title: tr('explorer.renameFailed'), message: String(e), danger: true });
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
					await alertDialog({ title: tr('explorer.createFailed'), message: isFile ? tr('explorer.createFileFailedMessage', { error: String(e) }) : tr('explorer.createDirFailedMessage', { error: String(e) }), danger: true });
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
			await alertDialog({ title: tr('explorer.revealFailed'), message: tr('explorer.revealFailedMessage', { error: String(e) }), danger: true });
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
				? tr('explorer.deleteMultiLabel', { count: targets.length, name: node.name })
				: `"${node.name}"`;
		const confirmed = await confirmDialog({
			title: tr('explorer.deleteFileTitle'),
			message: tr('explorer.deleteFileMessage', { label }),
			okLabel: tr('explorer.deleteFileLabel'),
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
				title: tr('explorer.deletePartialFailed'),
				message: tr('explorer.deletePartialFailedMessage', { count: errors.length, details: errors.join('\n') }),
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
		data-rg-tree-dir={node.is_dir ? 'true' : null}
		onpointerdown={onNodePointerDown}
		onpointermove={onNodePointerMove}
		onpointerup={(e) => void onNodePointerUp(e)}
		onpointercancel={onNodePointerCancel}
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
			<span class="w-4 h-4 shrink-0"></span>
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
						<span class="w-4 h-4 shrink-0"></span>
						<span class="w-4 h-4 flex items-center justify-center shrink-0 text-[var(--rg-accent)]">
							<PendingCreateIcon size={16} />
						</span>
						<input
							type="text"
							bind:this={editInput}
							bind:value={editValue}
							placeholder={editing === 'create-file' ? $t('explorer.newFileName') : $t('explorer.newFolderName')}
							class="flex-1 min-w-0 bg-[var(--rg-bg)] border border-[var(--rg-accent)]/60 outline-none rounded px-1 text-[13px] text-[var(--rg-fg)]"
							onkeydown={onEditKeydown}
							onblur={onEditBlur}
						/>
					</div>
				</div>
			{/if}
			{#if childrenPage.length > 0}
				{#each childrenPage as child (child.path)}
					<FileTree
						{columnId}
						node={child}
						depth={depth + 1}
						{expandedPaths}
						{selectedPath}
						{selectedPaths}
						{cutPaths}
						{refreshNonce}
						inheritedIgnored={isIgnored}
						{onSelect}
					/>
				{/each}
			{:else if hasLoaded && !editing}
				{#if childrenError}
					<div
						class="px-2 py-1 text-[12px] text-[var(--rg-accent)] cursor-pointer hover:underline"
						style="padding-left: {(depth + 1) * 8}px"
						onclick={() => void loadNextChildrenPage()}
						role="button"
						tabindex="0"
						onkeydown={(e) => e.key === 'Enter' && loadNextChildrenPage()}
					>
						{childrenError}
					</div>
				{:else}
					<div
						class="px-2 py-1 text-[12px] text-[var(--rg-fg-muted)]"
						style="padding-left: {(depth + 1) * 8}px"
					>
						{$t('explorer.emptyDirectory')}
					</div>
				{/if}
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
						<span class="w-4 h-4 shrink-0"></span>
						<span class="w-4 h-4 shrink-0"></span>
						{#if childrenLoading}
							{$t('explorer.loadingEllipsis')}
						{:else}
							{$t('explorer.loadMore', { remaining: Math.max(0, childrenTotalCount - childrenLoadedTotal) })}
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
	 * Gitignored row treatment — 50% opacity + muted foreground, no italic.
	 * Selection highlight still wins because the selected-row class is
	 * applied alongside `rg-tree-ignored` and raises foreground/background
	 * to the accent palette.
	 */
	.file-tree-node :global(.rg-tree-ignored) {
		opacity: 0.5;
		color: var(--rg-fg-muted);
	}
</style>
