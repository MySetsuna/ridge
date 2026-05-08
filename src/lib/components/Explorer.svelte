<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { ChevronRight, RefreshCw, FolderOpen, Save, Trash2, Terminal } from 'lucide-svelte';
	import { alertDialog, confirmDialog } from './RidgeDialog.svelte';
	import {
		fileExplorerStore,
		initFileExplorer,
		explorerWorkspaceGroups,
		toggleWorkspaceCollapsed,
		collapsedColumns,
		toggleColumnCollapsed,
		updateWorkspaceNames,
		flattenVisiblePaths,
		explorerClipboard,
		setExplorerClipboard,
		uniqueChildName,
		refreshColumnsCovering,
	} from '$lib/stores/fileExplorer';
	import { invoke, isTauri } from '@tauri-apps/api/core';
	import {
		paneCwdStore,
		terminalTitles,
		workspacesList,
		activeWorkspaceId,
		activePaneId,
		workspaceSaveInfoStore,
		refreshWorkspaceSaveInfo,
		saveWorkspaceToFile,
		deleteWorkspaceFile,
	} from '$lib/stores/paneTree';
	import { fileEditorStore, activeFile } from '$lib/stores/fileEditor';
	import { get } from 'svelte/store';
	import { tick } from 'svelte';
	import FileTree from './FileTree.svelte';
	import SaveWorkspaceDialog from './SaveWorkspaceDialog.svelte';
	import SidebarPluginRegion from './SidebarPluginRegion.svelte';
	import { overlayScroll } from '$lib/actions/overlayScroll';

	interface Props {
		workspaceId: string;
	}

	let { workspaceId }: Props = $props();

	// 慢加载（VS Code 风格）：col.loading 转 true 起 500ms 计时；到点仍未完成才挂进度条，
	// 数据到立刻撤掉。仅在没有缓存 tree 时启用——后台刷新保持静默。
	const SLOW_LOAD_THRESHOLD_MS = 500;
	let slowLoading = $state(new Set<string>());
	const slowTimers = new Map<string, ReturnType<typeof setTimeout>>();
	const slowPrevLoading = new Map<string, boolean>();
	function setSlow(id: string, on: boolean): void {
		if (slowLoading.has(id) === on) return;
		const next = new Set(slowLoading); on ? next.add(id) : next.delete(id); slowLoading = next;
	}
	function clearSlowTimer(id: string): void {
		const t = slowTimers.get(id);
		if (t !== undefined) { clearTimeout(t); slowTimers.delete(id); }
	}

	// --- Initial sync: all workspaces ---
	// §1.21 (2026-05-05 follow-up): drop `$terminalTitles` from this
	// effect's dependency set. It used to call `initFileExplorer(wsList,
	// titles)` which forwarded titles into `syncAllWorkspaces` →
	// `syncWithPaneCwds` (which always reallocates the columns array),
	// so every OSC title emit re-built the explorer and the file tree
	// flickered. Title sync now lives in the dedicated `updatePaneTitles`
	// effect below; this effect is purely for workspace structure.
	$effect(() => {
		const wsList = $workspacesList;
		if (wsList.length > 0) {
			initFileExplorer(wsList);
		}
	});

	// --- Reactive sync: re-run whenever any pane cwd changes ---
	//
	// §1.21 (2026-05-05): split title sync OUT of this $effect. Shells re-emit
	// OSC 0/1/2 on every prompt redraw (Ctrl+C, Enter, command lifecycle), so
	// $terminalTitles legitimately changes on every keystroke that produces
	// a prompt. Including titles here would re-call syncWithPaneCwds (which
	// always returns a new state object) → fileExplorerStore subscribers fire
	// → FileTree re-evaluates → user-visible flicker. Title-only changes now
	// flow through the dedicated `updatePaneTitles` effect below, which is
	// identity-preserving and never touches column structure / loadTree.
	//
	// 两条并行路径，用户强调"一定一定要确保 cwd 切换时文件树刷新"：
	//   1) $effect 走 Svelte 5 runes 自动订阅 —— 负责基础的 columns/paneIds 同步；
	//   2) 独立 paneCwdStore.subscribe —— 对每个真正发生变化的 key 强制目标列重载
	//      文件树（即使之前缓存过），彻底解决"切回老目录看不到新文件"的场景。
	$effect(() => {
		const cwds = $paneCwdStore;
		const wsList = $workspacesList;

		updateWorkspaceNames(wsList);

		for (const ws of wsList) {
			const workspaceCwds: Record<string, string> = {};
			for (const [key, cwd] of Object.entries(cwds)) {
				if (key.startsWith(`${ws.id}:`)) {
					const paneId = key.slice(ws.id.length + 1);
					workspaceCwds[paneId] = cwd;
				}
			}
			// Pass empty titles map — title sync runs in its own effect below.
			fileExplorerStore.syncWithPaneCwds(ws.id, workspaceCwds, {});
		}

		const cols = get(fileExplorerStore).columns;
		for (const col of cols) {
			// 首次加载（没有缓存树）或 needsRefresh（新 pane 加入老 cwd）都触发拉取。
			// 后者走"静默刷新"路径：保留旧树继续渲染，数据就绪后原子替换，避免空屏闪烁。
			if (!col.loading && (!col.tree || col.needsRefresh)) {
				void fileExplorerStore.loadTree(col.id);
			}
		}
	});

	// --- Title-only sync: identity-preserving, no column rebuild. ---
	// Runs whenever $terminalTitles changes. updatePaneTitles returns the
	// same state ref when no title actually differs from the cached one,
	// so prompt-redraw OSC events that happen to re-emit the previous
	// title are completely silent for fileExplorerStore subscribers.
	$effect(() => {
		const titles = $terminalTitles;
		const wsList = $workspacesList;
		for (const ws of wsList) {
			fileExplorerStore.updatePaneTitles(ws.id, titles);
		}
	});

	// loading 边沿监听：false→true 起计时器；true→false 撤标记；列消失也清理。
	$effect(() => {
		const cols = $fileExplorerStore.columns;
		const live = new Set<string>();
		for (const col of cols) {
			live.add(col.id);
			const id = col.id;
			const prev = slowPrevLoading.get(id) ?? false;
			const now = col.loading && !col.tree;
			if (now && !prev) {
				clearSlowTimer(id);
				slowTimers.set(id, setTimeout(() => { slowTimers.delete(id); setSlow(id, true); }, SLOW_LOAD_THRESHOLD_MS));
			} else if (!now && prev) { clearSlowTimer(id); setSlow(id, false); }
			slowPrevLoading.set(id, now);
		}
		for (const id of Array.from(slowPrevLoading.keys())) {
			if (live.has(id)) continue;
			clearSlowTimer(id); setSlow(id, false); slowPrevLoading.delete(id);
		}
	});

	/**
	 * Reveal the currently active editor file in the tree (VS Code
	 * "Auto Reveal" behaviour). Triggers when the editor's active tab changes;
	 * finds the column whose cwd is a prefix of the file, expands every
	 * ancestor directory, selects the node, and scrolls its row into view.
	 *
	 * Deliberately no-ops when the file is already selected to avoid thrashing
	 * scroll position while the user navigates within the tree.
	 */
	async function revealInTree(path: string): Promise<void> {
		const state = get(fileExplorerStore);
		// Column = the one whose cwd is the longest prefix of path.
		// Normalise separators to handle Windows vs posix mixes.
		const normalise = (s: string) => s.replace(/\\/g, '/').replace(/\/+$/, '');
		const np = normalise(path);
		let best: { col: (typeof state.columns)[number]; len: number } | null = null;
		for (const col of state.columns) {
			const nc = normalise(col.cwd);
			if (nc && (np === nc || np.startsWith(nc + '/'))) {
				if (!best || nc.length > best.len) best = { col, len: nc.length };
			}
		}
		if (!best) return;
		const col = best.col;
		if (col.selectedPath === path) return;

		// Expand every ancestor directory. Paths can mix separators — we
		// re-emit using the column's cwd separator style (same trick as the
		// FileTree joinChild helper). Batched via `expandMany` so the reveal
		// triggers one store update + one persistence write, instead of N.
		const colSep = col.cwd.includes('\\') && !col.cwd.includes('/') ? '\\' : '/';
		const rel = np.slice(best.len).replace(/^\//, '');
		if (rel) {
			const parts = rel.split('/');
			const ancestors: string[] = [];
			let cursor = normalise(col.cwd);
			for (let i = 0; i < parts.length - 1; i += 1) {
				cursor = `${cursor}/${parts[i]}`;
				// Re-express with the column's separator style.
				const nativeAncestor = cursor.replace(/\//g, colSep);
				ancestors.push(nativeAncestor);
			}
			fileExplorerStore.expandMany(col.id, ancestors);
		}

		fileExplorerStore.setSelectedPath(col.id, path);

		// Wait for the reactive render cycle so FileTree has (a) re-rendered
		// with the new selection and (b) lazy-loaded any freshly-expanded
		// directories. Then find the target row and scroll it into view.
		await tick();
		// The lazy FileTree.loadChildren resolves in a microtask, so give it
		// one more frame for dynamic children to mount.
		await new Promise((r) => requestAnimationFrame(() => r(null)));
		const btn = document.querySelector<HTMLButtonElement>(
			`button[data-rg-tree-column="${CSS.escape(col.id)}"][data-rg-tree-path="${CSS.escape(path)}"]`
		);
		btn?.scrollIntoView({ block: 'nearest' });
	}

	/**
	 * Subscribe to the active editor file. Deliberately *not* a $effect reading
	 * $activeFile inside the reactive graph — we only want the `path` diff to
	 * trigger the reveal, and only after initial mount (to avoid jumping when
	 * the app first launches with a saved-state file still active).
	 */
	let prevActivePath: string | null = null;
	let unsubActiveFile: (() => void) | undefined;
	onMount(() => {
		unsubActiveFile = activeFile.subscribe((f) => {
			const next = f?.path ?? null;
			if (next && next !== prevActivePath) {
				void revealInTree(next);
			}
			prevActivePath = next;
		});
	});
	onDestroy(() => {
		unsubActiveFile?.();
	});

	// 兜底：直接订阅 paneCwdStore，逐键比对上一次值，仅当「既有 pane 的 cwd 发生变化」
	// （即用户在 shell 里执行了 cd）才强制 loadTree。
	// 新 pane 加入（key 不在 prevCwdSnapshot）的情况由 syncWithPaneCwds + needsRefresh
	// 负责，这里不处理，避免已有缓存 tree 被重复扫描。
	let prevCwdSnapshot: Record<string, string> = {};
	let unsubPaneCwd: (() => void) | undefined;
	onMount(() => {
		unsubPaneCwd = paneCwdStore.subscribe((cwds) => {
			const changedCwds = new Set<string>();
			for (const [key, cwd] of Object.entries(cwds)) {
				// Only count as a change if the pane already existed AND its cwd changed.
				// New pane additions (key absent in prevCwdSnapshot) are handled by
				// syncWithPaneCwds / needsRefresh; triggering loadTree here would bypass
				// the cache and re-scan an already-loaded directory.
				if (key in prevCwdSnapshot && prevCwdSnapshot[key] !== cwd) {
					changedCwds.add(cwd);
				}
			}
			prevCwdSnapshot = { ...cwds };
			if (changedCwds.size === 0) return;
			// 延迟一个微任务，让 syncWithPaneCwds 先跑（它可能已经创建了新 column）。
			queueMicrotask(() => {
				const state = get(fileExplorerStore);
				for (const col of state.columns) {
					// Only reload when this cwd changed AND the column has no cached
					// tree yet. If another pane already has a loaded tree for this
					// cwd, reuse it — the user explicitly asked not to re-scan
					// directories that already have cached resources.
					if (changedCwds.has(col.cwd) && !col.tree) {
						void fileExplorerStore.loadTree(col.id);
					}
				}
			});
		});
	});
	onDestroy(() => {
		unsubPaneCwd?.();
		for (const h of slowTimers.values()) clearTimeout(h);
		slowTimers.clear(); slowPrevLoading.clear();
	});

	// 刷新按钮 in-flight 标记 —— spinner 视觉反馈靠这个 Set；用 reassign
	// 触发 Svelte 5 reactivity（mutate Set 不会通知 $state）。
	let refreshingColumns = $state<Set<string>>(new Set());

	async function handleRefresh(columnId: string): Promise<void> {
		const next = new Set(refreshingColumns);
		next.add(columnId);
		refreshingColumns = next;
		try {
			await fileExplorerStore.loadTree(columnId);
		} finally {
			const out = new Set(refreshingColumns);
			out.delete(columnId);
			refreshingColumns = out;
		}
	}

	/**
	 * Click on a file-tree node. `_columnId` intentionally ignored — we derive
	 * the column from the node's path so multi-select stays scoped to one column
	 * even when tree re-renders change identities.
	 *
	 * Modifier contract (matches VS Code):
	 *   Plain click      → replace selection with just `path`; anchor = path
	 *   Shift + click    → range from anchor to path (primary = path; anchor unchanged)
	 *   Ctrl / Cmd click → toggle membership of `path`; primary = path; anchor = path
	 */
	function handleFileSelect(
		path: string,
		_columnId: string,
		isDir: boolean,
		modifiers?: { shift: boolean; ctrl: boolean; meta: boolean }
	) {
		const state = get(fileExplorerStore);
		const col = state.columns.find((c) => c.id === _columnId);
		if (!col) return;
		const mod = modifiers ?? { shift: false, ctrl: false, meta: false };
		const additive = mod.ctrl || mod.meta;

		if (mod.shift && col.anchorPath) {
			// Range selection: flatten visible, slice [anchor..path] inclusive.
			const flat = flattenVisiblePaths(col);
			const aIdx = flat.indexOf(col.anchorPath);
			const bIdx = flat.indexOf(path);
			if (aIdx >= 0 && bIdx >= 0) {
				const [lo, hi] = aIdx <= bIdx ? [aIdx, bIdx] : [bIdx, aIdx];
				const range = flat.slice(lo, hi + 1);
				fileExplorerStore.setSelection(_columnId, {
					paths: range,
					primary: path,
					// Shift click/arrow keeps anchor pinned. Pass `undefined`
					// so setSelection preserves existing anchor.
					anchor: undefined,
				});
				// Files: open in editor on shift-click of a file (VS Code behaviour
				// is actually no-open for shift; we match that for files and dirs).
				return;
			}
		}
		if (additive) {
			const next = new Set(col.selectedPaths);
			if (next.has(path)) next.delete(path);
			else next.add(path);
			// Ctrl-click updates anchor to the clicked item (new range start).
			fileExplorerStore.setSelection(_columnId, {
				paths: next,
				primary: path,
				anchor: path,
			});
			return;
		}
		// Plain click → single selection.
		fileExplorerStore.setSelectedPath(_columnId, path);
		// Open file into the editor on plain click (dirs already toggled by FileTree).
		if (!isDir) void fileEditorStore.openFile(path);
	}

	// Root-level keyboard nav (ArrowUp/Down/Home/End) lives on `.explorer` so it can
	// cross FileTree node boundaries; per-node keys (Enter/F2/Delete/Arrow Left/Right)
	// stay on the node button in FileTree.svelte. See `handleRootKeydown` below.

	/** Paste clipboard into the target dir (selected dir, or parent of selected file). */
	async function pasteClipboard(): Promise<void> {
		if (!isTauri()) return;
		const clip = get(explorerClipboard);
		if (!clip || clip.paths.length === 0) return;

		const state = get(fileExplorerStore);
		// Find the active column & target dir.
		let col = state.columns.find((c) => c.selectedPath);
		if (!col) col = state.columns.find((c) => c.tree);
		if (!col) return;

		let targetDir: string | null = null;
		const primary = col.selectedPath;
		if (primary) {
			// If primary is a dir, paste into it; if a file, paste into its parent.
			// We detect dir by walking the cached tree — no extra IPC.
			const seenDirs = new Set<string>();
			const walkDirs = (n: typeof col.tree) => {
				if (!n) return;
				if (n.is_dir) seenDirs.add(n.path);
				if (n.children) for (const child of n.children) walkDirs(child);
			};
			walkDirs(col.tree);
			if (seenDirs.has(primary)) targetDir = primary;
			else {
				// Parent of the file.
				targetDir = primary.replace(/[\\/][^\\/]+[\\/]*$/, '') || primary;
			}
		} else {
			targetDir = col.cwd;
		}
		if (!targetDir) return;

		// Build existing-name set from the target dir's children (tree cache).
		const existingInTarget = new Set<string>();
		const findDirChildren = (n: typeof col.tree): string[] | null => {
			if (!n) return null;
			if (n.path === targetDir) return (n.children ?? []).map((c) => c.path);
			if (n.children) {
				for (const child of n.children) {
					const r = findDirChildren(child);
					if (r) return r;
				}
			}
			return null;
		};
		for (const p of findDirChildren(col.tree) ?? []) existingInTarget.add(p);

		const cmd = clip.mode === 'copy' ? 'copy_path' : 'move_path';
		const errors: string[] = [];
		for (const from of clip.paths) {
			const name = from.split(/[\\/]/).pop() || 'untitled';
			const unique = uniqueChildName(targetDir, name, existingInTarget);
			const sep = targetDir.includes('\\') && !targetDir.includes('/') ? '\\' : '/';
			const to = `${targetDir.replace(/[\\/]+$/, '')}${sep}${unique}`;
			existingInTarget.add(to);
			try {
				await invoke(cmd, { from, to });
			} catch (e) {
				errors.push(`${from}: ${e}`);
			}
		}
		// Consume the clipboard on successful cut; copy stays armed so repeat-paste works.
		if (clip.mode === 'cut' && errors.length < clip.paths.length) {
			setExplorerClipboard(null);
		}
		// Refresh every column that had the target dir in its cached tree —
		// fixes the "two panes at same cwd see stale tree after paste" case.
		// Source dir (parent of each cut path) also refreshed so the row
		// disappears from cut columns on the move branch.
		await refreshColumnsCovering(targetDir);
		if (clip.mode === 'cut') {
			const sourceDirs = new Set<string>(
				clip.paths.map((p) => p.replace(/[\\/][^\\/]+[\\/]*$/, '') || p)
			);
			for (const d of sourceDirs) await refreshColumnsCovering(d);
		}
		if (errors.length > 0) {
			await alertDialog({ title: '粘贴失败', message: `${errors.length} 项粘贴失败:\n${errors.join('\n')}`, danger: true });
		}
	}

	function handleRootKeydown(e: KeyboardEvent): void {
		if (e.isComposing) return;

		// Clipboard shortcuts. Don't fire when the user is typing in an <input>
		// (inline rename); those inputs live inside FileTree and will stopPropagation
		// only for their own keys, but we still gate on activeElement to be safe.
		const inEditable =
			document.activeElement instanceof HTMLInputElement ||
			document.activeElement instanceof HTMLTextAreaElement;
		if (!inEditable && (e.ctrlKey || e.metaKey)) {
			if (e.key === 'c' || e.key === 'C' || e.key === 'x' || e.key === 'X') {
				const state = get(fileExplorerStore);
				const col = state.columns.find((c) => c.selectedPath);
				if (!col) return;
				const paths = Array.from(col.selectedPaths);
				if (paths.length === 0) return;
				const mode: 'copy' | 'cut' = e.key.toLowerCase() === 'c' ? 'copy' : 'cut';
				setExplorerClipboard({ paths, mode });
				// Mirror to OS clipboard so Ctrl+V into a terminal / file manager
				// pastes a usable list of paths. Only on copy (cut leaves OS
				// clipboard alone because "cut" semantics don't map to shell
				// clipboards and we don't want to accidentally move on external
				// paste).
				if (mode === 'copy' && isTauri()) {
					void (async () => {
						try {
							const { writeText } = await import('@tauri-apps/plugin-clipboard-manager');
							await writeText(paths.join('\n'));
						} catch (err) {
							console.warn('[explorer] clipboard writeText failed', err);
						}
					})();
				}
				e.preventDefault();
				return;
			}
			if (e.key === 'v' || e.key === 'V') {
				e.preventDefault();
				void pasteClipboard();
				return;
			}
		}
		// Escape clears the clipboard "cut" visual (like VS Code).
		if (!inEditable && e.key === 'Escape') {
			const clip = get(explorerClipboard);
			if (clip?.mode === 'cut') {
				setExplorerClipboard(null);
				e.preventDefault();
				return;
			}
		}

		if (
			e.key !== 'ArrowUp' &&
			e.key !== 'ArrowDown' &&
			e.key !== 'Home' &&
			e.key !== 'End'
		) {
			return;
		}
		const activeEl = document.activeElement as HTMLElement | null;
		const fromPath = activeEl?.dataset?.wfTreePath ?? null;
		const fromColumn = activeEl?.dataset?.wfTreeColumn ?? null;

		const state = get(fileExplorerStore);
		// Prefer the column of the currently focused node; otherwise first loaded column.
		const col =
			(fromColumn && state.columns.find((c) => c.id === fromColumn)) ||
			state.columns.find((c) => c.tree);
		if (!col) return;

		const flat = flattenVisiblePaths(col);
		if (flat.length === 0) return;

		const currentIdx = fromPath ? flat.indexOf(fromPath) : -1;
		let nextIdx = currentIdx;
		switch (e.key) {
			case 'ArrowDown':
				nextIdx = currentIdx < 0 ? 0 : Math.min(flat.length - 1, currentIdx + 1);
				break;
			case 'ArrowUp':
				nextIdx = currentIdx < 0 ? flat.length - 1 : Math.max(0, currentIdx - 1);
				break;
			case 'Home':
				nextIdx = 0;
				break;
			case 'End':
				nextIdx = flat.length - 1;
				break;
		}
		if (nextIdx === currentIdx && currentIdx >= 0) return;
		const targetPath = flat[nextIdx];
		e.preventDefault();
		if (e.shiftKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown') && col.anchorPath) {
			// Shift + Arrow extends the selection from the existing anchor to
			// the new cursor position (matches VS Code's tree Shift+Arrow).
			const aIdx = flat.indexOf(col.anchorPath);
			if (aIdx >= 0) {
				const [lo, hi] = aIdx <= nextIdx ? [aIdx, nextIdx] : [nextIdx, aIdx];
				fileExplorerStore.setSelection(col.id, {
					paths: flat.slice(lo, hi + 1),
					primary: targetPath,
					anchor: undefined,
				});
			} else {
				fileExplorerStore.setSelectedPath(col.id, targetPath);
			}
		} else {
			fileExplorerStore.setSelectedPath(col.id, targetPath);
		}
		// Focus the target node's button so per-node keys keep working from
		// the new position (and the outline-focus ring draws there).
		queueMicrotask(() => {
			const btn = document.querySelector<HTMLButtonElement>(
				`button[data-rg-tree-column="${CSS.escape(col.id)}"][data-rg-tree-path="${CSS.escape(targetPath)}"]`
			);
			btn?.focus();
			btn?.scrollIntoView({ block: 'nearest' });
		});
	}

	function getPaneLabel(paneId: string, paneTitles: Record<string, string>): string {
		return paneTitles[paneId] || $terminalTitles[paneId] || '终端';
	}

	const totalColumns = $derived($explorerWorkspaceGroups.reduce((n, g) => n + g.columns.length, 0));


	// ─── 保存 / 删除 / 打开 .ridge 工作区文件 ───────────────────────────────────
	$effect(() => {
		// Keep save-info badges in sync when the workspaces list changes (create/close/rename).
		if ($workspacesList.length > 0) {
			void refreshWorkspaceSaveInfo();
		}
	});

	let saveDialogOpen = $state(false);
	let saveTargetWorkspaceId = $state<string>('');
	let saveDefaultName = $state('');

	function openSaveDialog(wsId: string, currentName: string | undefined) {
		saveTargetWorkspaceId = wsId;
		saveDefaultName = currentName?.trim() || '';
		saveDialogOpen = true;
	}

	async function handleSaveConfirm(name: string, path: string | null) {
		const target = saveTargetWorkspaceId;
		if (!target) return;
		await saveWorkspaceToFile(target, name, path ?? undefined);
	}

	async function handleDelete(wsId: string, filePath: string | null | undefined) {
		const confirmed = await confirmDialog({
			title: '删除工作区文件',
			message: `从磁盘删除已保存的工作区文件？\n\n${filePath || '(未知路径)'}\n\n此操作只删除 .ridge 文件，不会关闭当前工作区。`,
			okLabel: '删除',
			danger: true,
		});
		if (!confirmed) return;
		try {
			await deleteWorkspaceFile(wsId);
		} catch (e) {
			await alertDialog({ title: '删除失败', message: String(e), danger: true });
		}
	}

</script>

<SaveWorkspaceDialog
	bind:open={saveDialogOpen}
	defaultName={saveDefaultName}
	onConfirm={handleSaveConfirm}
	onCancel={() => (saveDialogOpen = false)}
/>

<!--
  overlay 滚动条：host 不做 overflow，交给 overlayscrollbars；content 自然堆叠。
  之前 `rg-scroll-overlay` + `overflow-y-auto` 会借助 `scrollbar-gutter: stable`
  常驻 10px gutter —— 改用 overlayscrollbars 后滚动条绝对定位浮在上方，
  "显示 / 隐藏" 不再改变内容宽度；hover/滚动时显形，空闲时淡出。
-->
<!-- tabindex=0 + onkeydown 让 Explorer 根节点可以接 ArrowUp/Down/Home/End —— 每个
     FileNode 按钮自己能聚焦，但跨节点导航需要一层 coordinator；这里承担这个角色。 -->
<div
	class="explorer flex h-full flex-col"
	data-testid="file-tree"
	tabindex="-1"
	use:overlayScroll
	onkeydown={handleRootKeydown}
	role="tree"
>
	{#if totalColumns === 0}
		<div class="flex-1 flex items-center justify-center">
			<div class="text-center">
				<FolderOpen class="mx-auto h-12 w-12 text-[var(--rg-fg-muted)] mb-4" />
				<p class="text-[13px] text-[var(--rg-fg-muted)]">无活动终端</p>
				<p class="text-[12px] text-[var(--rg-fg-muted)] mt-1">
					打开终端后将在此显示文件树
				</p>
			</div>
		</div>
	{:else}
		{#each $explorerWorkspaceGroups as group (group.workspaceId)}
			{@const info = $workspaceSaveInfoStore[group.workspaceId]}
			<!-- ══ Workspace header row ══ -->
			<div
				class="explorer-workspace border-b border-[var(--rg-border)] last:border-b-0"
			>
				<div
					class="group/ws sticky top-0 z-20 flex items-center h-8 px-2 gap-1.5 cursor-pointer select-none backdrop-blur-md
						{group.workspaceId === $activeWorkspaceId
							? 'bg-[var(--rg-accent)]/20 text-[var(--rg-fg)]'
							: 'bg-[var(--rg-surface-2)]/92 text-[var(--rg-fg-muted)] hover:bg-[var(--rg-surface)]'}"
					role="button"
					tabindex="0"
					onclick={() => toggleWorkspaceCollapsed(group.workspaceId)}
					onkeydown={(e) => e.key === 'Enter' && toggleWorkspaceCollapsed(group.workspaceId)}
				>
					<!-- Workspace collapse chevron -->
					<ChevronRight
						class="h-3.5 w-3.5 shrink-0 transition-transform duration-150
							{group.collapsed ? '' : 'rotate-90'}
							{group.workspaceId === $activeWorkspaceId ? 'text-[var(--rg-accent)]' : 'text-[var(--rg-fg-muted)]'}"
					/>

					<!-- Workspace name -->
					<span
						class="flex-1 text-[11px] font-semibold uppercase tracking-wider truncate
							{group.workspaceId === $activeWorkspaceId ? 'text-[var(--rg-accent)]' : 'text-[var(--rg-fg-muted)]'}"
					>
						{group.workspaceName}
					</span>

					<!-- Column count badge -->
					<span class="text-[10px] text-[var(--rg-fg-muted)] shrink-0">
						{group.columns.length}
					</span>

					<!-- Save (unsaved only) / Delete (saved only) action -->
					{#if !info?.file_path}
						<button
							type="button"
							class="flex h-5 w-5 shrink-0 items-center justify-center rounded text-[var(--rg-fg-muted)] opacity-0 group-hover/ws:opacity-100 hover:!text-[var(--rg-accent)] hover:bg-[var(--rg-surface)] transition-all"
							title="保存工作区为 .ridge 文件"
							onclick={(e) => {
								e.stopPropagation();
								openSaveDialog(group.workspaceId, group.workspaceName);
							}}
						>
							<Save class="h-3 w-3" />
						</button>
					{:else}
						<button
							type="button"
							class="flex h-5 w-5 shrink-0 items-center justify-center rounded text-[var(--rg-fg-muted)] opacity-0 group-hover/ws:opacity-100 hover:!text-red-400 hover:bg-[var(--rg-surface)] transition-all"
							title={`删除已保存的 .ridge 文件\n${info.file_path}`}
							onclick={(e) => {
								e.stopPropagation();
								void handleDelete(group.workspaceId, info.file_path);
							}}
						>
							<Trash2 class="h-3 w-3" />
						</button>
					{/if}
				</div>

				<!-- Workspace-scope plugin region — mounted once per workspace,
				     directly beneath its header. Stubs today; Claude session
				     summary / workspace memo land here in future rounds. -->
				{#if !group.collapsed}
					<SidebarPluginRegion scope="workspace" workspaceId={group.workspaceId} />
				{/if}

				<!-- ══ CWD groups under this workspace ══ -->
				{#if !group.collapsed}
					{#each group.columns as col (col.id)}
						{@const isColCollapsed = $collapsedColumns.has(col.id)}
						{@const cwdSegments = col.cwd.replace(/\\/g, '/').split('/').filter(Boolean)}
						{@const cwdLeaf = cwdSegments[cwdSegments.length - 1] ?? col.cwd}
						{@const cwdParent = cwdSegments.slice(0, -1).join('/')}
						<!-- T18：终端节点（cwd 卡片）— 工作区下的中间层，可折叠隐藏文件树 -->
						<div class="explorer-section group/col border-t border-[var(--rg-border)]/50">
							<!-- ── CWD 头：chevron + 路径 + pane 数 + 刷新 ── -->
							<div
								class="flex items-center gap-1 h-7 px-2 cursor-pointer select-none transition-colors {isColCollapsed
									? 'bg-[var(--rg-surface-2)]/60 hover:bg-[var(--rg-surface-2)]/80'
									: 'bg-[var(--rg-surface)]/40 hover:bg-[var(--rg-surface)]/70'}"
								role="button"
								tabindex="0"
								title={col.cwd}
								onclick={() => toggleColumnCollapsed(col.id)}
								onkeydown={(e) => e.key === 'Enter' && toggleColumnCollapsed(col.id)}
							>
								<ChevronRight
									class="h-3 w-3 shrink-0 text-[var(--rg-fg-muted)] transition-transform duration-150 {isColCollapsed ? '' : 'rotate-90'}"
								/>
								<Terminal class="h-3 w-3 shrink-0 text-[var(--rg-accent)]" />
								<span class="flex-1 min-w-0 truncate text-[11px]">
									{#if cwdParent}
										<span class="text-[var(--rg-fg-muted)]/60">{cwdParent}/</span>
									{/if}
									<span class="text-[var(--rg-fg)] font-medium">{cwdLeaf}</span>
								</span>
								<span class="text-[9px] text-[var(--rg-fg-muted)] shrink-0 font-mono">
									{col.paneIds.length}
								</span>
								<button
									type="button"
									class="flex h-4 w-4 shrink-0 items-center justify-center rounded text-[var(--rg-fg-muted)] {refreshingColumns.has(col.id)
										? 'opacity-100'
										: 'opacity-0 group-hover/col:opacity-60 hover:!opacity-100'} hover:bg-[var(--rg-accent)]/20 hover:text-[var(--rg-fg)] transition-all disabled:cursor-not-allowed"
									disabled={refreshingColumns.has(col.id)}
									onclick={(e) => {
										e.stopPropagation();
										void handleRefresh(col.id);
									}}
									title={`刷新 ${col.cwd}`}
								>
									<RefreshCw class="h-2.5 w-2.5 {refreshingColumns.has(col.id) ? 'animate-spin' : ''}" />
								</button>
							</div>

							<!-- ── pane 标签条：折叠时也显示，点击切 active pane ── -->
							{#if col.paneIds.length > 0}
								<div class="flex flex-wrap items-center gap-1 px-2 py-1 bg-[var(--rg-surface)]/20 border-t border-[var(--rg-border)]/30">
									{#each col.paneIds as pid (pid)}
										{@const isActive = $activePaneId === pid}
										<button
											type="button"
											class="flex items-center gap-1 h-4 px-1.5 rounded text-[10px] transition-colors {isActive
												? 'bg-[var(--rg-accent)]/25 text-[var(--rg-accent)] border border-[var(--rg-accent)]/40'
												: 'bg-[var(--rg-surface-2)]/60 text-[var(--rg-fg-muted)] border border-[var(--rg-border)] hover:text-[var(--rg-fg)] hover:border-[var(--rg-border-bright)]'}"
											title={col.paneTitles[pid] || pid}
											onclick={() => activePaneId.set(pid)}
										>
											<Terminal class="h-2.5 w-2.5 shrink-0" />
											<span class="truncate max-w-[110px]">{col.paneTitles[pid] || pid.slice(0, 6)}</span>
										</button>
									{/each}
								</div>
							{/if}

							{#if !isColCollapsed}
								<!-- 慢加载提示：>500ms 未完成且无缓存树时才出现；数据到立刻撤掉。 -->
								{#if slowLoading.has(col.id)}
									<div class="explorer-progress" role="progressbar" aria-busy="true" aria-label="加载中"></div>
								{/if}

								<!-- File tree body: cwd 下文件平铺。 -->
								<div class="relative explorer-body py-0.5 {group.workspaceId !== $activeWorkspaceId ? "max-h-[32vh] overflow-y-auto rg-scroll" : ""}">
									{#if col.tree}
										{#if (col.tree.children ?? []).length > 0}
											{#each col.tree.children ?? [] as child (child.path)}
												<FileTree
													columnId={col.id}
													node={child}
													depth={0}
													expandedPaths={col.expandedPaths}
													selectedPath={col.selectedPath}
													selectedPaths={col.selectedPaths}
													refreshNonce={col.refreshNonce}
													cutPaths={$explorerClipboard?.mode === 'cut'
														? new Set($explorerClipboard.paths)
														: undefined}
													onSelect={(path, isDir, mods) =>
														handleFileSelect(path, col.id, isDir, mods)}
												/>
											{/each}
										{:else}
											<div class="px-4 py-2 text-[12px] text-[var(--rg-fg-muted)]">空目录</div>
										{/if}
									{/if}
								</div>

								<!-- Plugin region: one instance per paneId for correct state scoping. -->
								{#each col.paneIds as pid (pid)}
									<SidebarPluginRegion scope="pane" workspaceId={col.workspaceId} paneId={pid} cwd={col.cwd} />
								{/each}
							{/if}
						</div>
					{/each}
				{/if}
			</div>
		{/each}
	{/if}
</div>

<style>
	/* 滚动条样式由 `use:overlayScroll` action（`src/lib/actions/overlayScroll.ts`）
	   提供，不在本组件本地覆盖。rg-os-theme 主题见 `app.css`。 */

	/* 慢加载不定式进度条：2px、accent 半透明轨 + 30% 宽 transform 滑块，纯 compositor。 */
	.explorer-progress {
		position: relative; height: 2px; width: 100%; overflow: hidden;
		background: color-mix(in oklab, var(--rg-accent) 25%, transparent);
	}
	.explorer-progress::before {
		content: ''; position: absolute; top: 0; left: 0; height: 100%; width: 30%;
		background: var(--rg-accent); will-change: transform;
		animation: explorer-progress-slide 1.1s cubic-bezier(0.4, 0, 0.2, 1) infinite;
	}
	@keyframes explorer-progress-slide {
		0% { transform: translateX(-100%); } 100% { transform: translateX(400%); }
	}
	@media (prefers-reduced-motion: reduce) {
		.explorer-progress::before { animation-duration: 2.4s; }
	}
</style>
