<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { ChevronRight, RefreshCw, FolderOpen, Save, Trash2 } from 'lucide-svelte';
	import {
		fileExplorerStore,
		initFileExplorer,
		explorerWorkspaceGroups,
		toggleWorkspaceCollapsed,
		updateWorkspaceNames,
	} from '$lib/stores/fileExplorer';
	import {
		paneCwdStore,
		terminalTitles,
		workspacesList,
		activeWorkspaceId,
		workspaceSaveInfoStore,
		refreshWorkspaceSaveInfo,
		saveWorkspaceToFile,
		deleteWorkspaceFile,
	} from '$lib/stores/paneTree';
	import { fileEditorStore } from '$lib/stores/fileEditor';
	import { get } from 'svelte/store';
	import FileTree from './FileTree.svelte';
	import SaveWorkspaceDialog from './SaveWorkspaceDialog.svelte';
	import { scrollOverlay } from '$lib/actions/scrollOverlay';

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
	//
	// 两条并行路径，用户强调"一定一定要确保 cwd 切换时文件树刷新"：
	//   1) $effect 走 Svelte 5 runes 自动订阅 —— 负责基础的 columns/paneIds 同步；
	//   2) 独立 paneCwdStore.subscribe —— 对每个真正发生变化的 key 强制目标列重载
	//      文件树（即使之前缓存过），彻底解决"切回老目录看不到新文件"的场景。
	$effect(() => {
		const cwds = $paneCwdStore;
		const titles = $terminalTitles;
		const wsList = $workspacesList;

		updateWorkspaceNames(wsList);

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

		const cols = get(fileExplorerStore).columns;
		for (const col of cols) {
			if (!col.tree && !col.loading) {
				void fileExplorerStore.loadTree(col.id);
			}
		}
	});

	// 兜底：直接订阅 paneCwdStore，逐键比对上一次值，凡是「某 pane 的 cwd 发生变化」
	// 就把目标列强制 loadTree —— 这样不依赖 syncWithPaneCwds 的 "new joiner" 判定，
	// 任何 shell 的 cd 都一定触发文件树重拉。
	let prevCwdSnapshot: Record<string, string> = {};
	let unsubPaneCwd: (() => void) | undefined;
	onMount(() => {
		unsubPaneCwd = paneCwdStore.subscribe((cwds) => {
			const changedCwds = new Set<string>();
			for (const [key, cwd] of Object.entries(cwds)) {
				if (prevCwdSnapshot[key] !== cwd) changedCwds.add(cwd);
			}
			for (const key of Object.keys(prevCwdSnapshot)) {
				if (!(key in cwds)) changedCwds.add(prevCwdSnapshot[key]);
			}
			prevCwdSnapshot = { ...cwds };
			if (changedCwds.size === 0) return;
			// 延迟一个微任务，让 syncWithPaneCwds 先跑（它可能已经创建了新 column）。
			queueMicrotask(() => {
				const state = get(fileExplorerStore);
				for (const col of state.columns) {
					if (changedCwds.has(col.cwd)) {
						void fileExplorerStore.loadTree(col.id);
					}
				}
			});
		});
	});
	onDestroy(() => {
		unsubPaneCwd?.();
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

	function handleRefresh(columnId: string) {
		void fileExplorerStore.loadTree(columnId);
	}

	function handleFileSelect(path: string, _columnId: string, isDir: boolean) {
		if (isDir) return;
		void fileEditorStore.openFile(path);
	}

	function getPaneLabel(paneId: string, paneTitles: Record<string, string>): string {
		return paneTitles[paneId] || $terminalTitles[paneId] || '终端';
	}

	const totalColumns = $derived($explorerWorkspaceGroups.reduce((n, g) => n + g.columns.length, 0));

	// ─── 保存 / 删除 / 打开 .wind 工作区文件 ───────────────────────────────────
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
		const confirmed = confirm(
			`从磁盘删除已保存的工作区文件？\n\n${filePath || '(未知路径)'}\n\n此操作只删除 .wind 文件，不会关闭当前工作区。`
		);
		if (!confirmed) return;
		try {
			await deleteWorkspaceFile(wsId);
		} catch (e) {
			alert(`删除失败: ${e}`);
		}
	}

</script>

<SaveWorkspaceDialog
	bind:open={saveDialogOpen}
	defaultName={saveDefaultName}
	onConfirm={handleSaveConfirm}
	onCancel={() => (saveDialogOpen = false)}
/>

<div class="explorer wf-scroll-overlay flex h-full flex-col overflow-y-auto" data-testid="file-tree" use:scrollOverlay>
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
			{@const info = $workspaceSaveInfoStore[group.workspaceId]}
			<!-- ══ Workspace header row ══ -->
			<div
				class="explorer-workspace border-b border-[var(--wf-border)] last:border-b-0"
			>
				<div
					class="group/ws sticky top-0 z-20 flex items-center h-8 px-2 gap-1.5 cursor-pointer select-none backdrop-blur-md
						{group.workspaceId === $activeWorkspaceId
							? 'bg-[var(--wf-accent)]/20 text-[var(--wf-fg)]'
							: 'bg-[var(--wf-surface-2)]/92 text-[var(--wf-fg-muted)] hover:bg-[var(--wf-surface)]'}"
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

					<!-- Save (unsaved only) / Delete (saved only) action -->
					{#if !info?.file_path}
						<button
							type="button"
							class="flex h-5 w-5 shrink-0 items-center justify-center rounded text-[var(--wf-fg-muted)] opacity-0 group-hover/ws:opacity-100 hover:!text-[var(--wf-accent)] hover:bg-[var(--wf-surface)] transition-all"
							title="保存工作区为 .wind 文件"
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
							class="flex h-5 w-5 shrink-0 items-center justify-center rounded text-[var(--wf-fg-muted)] opacity-0 group-hover/ws:opacity-100 hover:!text-red-400 hover:bg-[var(--wf-surface)] transition-all"
							title={`删除已保存的 .wind 文件\n${info.file_path}`}
							onclick={(e) => {
								e.stopPropagation();
								void handleDelete(group.workspaceId, info.file_path);
							}}
						>
							<Trash2 class="h-3 w-3" />
						</button>
					{/if}
				</div>

				<!-- ══ CWD groups under this workspace ══ -->
				{#if !group.collapsed}
					{#each group.columns as col (col.id)}
						<div class="explorer-section border-t border-[var(--wf-border)]/50">
							<!-- Compact CWD section header -->
							<div
								class="group sticky top-8 z-10 flex items-center h-7 px-2 gap-1 cursor-pointer select-none bg-[var(--wf-surface-2)]/88 backdrop-blur-md hover:bg-[var(--wf-surface)] transition-colors"
								role="button"
								tabindex="0"
								onclick={() => toggleColumnCollapse(col.id)}
								onkeydown={(e) => e.key === 'Enter' && toggleColumnCollapse(col.id)}
								title={col.cwd}
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

							</div>

							<!-- Collapsible file tree body.
							     文件树不再自己开滚动域：内容按自然高度铺开，由外层 .explorer
							     的唯一滚动条统一承载。sticky 工作区/终端头就始终是"浮在真正
							     被滚动的那一层内容上"，不会出现"文件树自己滚而外层 sticky 盖
							     住它顶部"的错位感（以前 `max-height` + `overflow-y-auto`
							     会产生这种双重滚动）。 -->
							{#if !collapsedColumns.has(col.id)}
								<div class="explorer-body py-0.5">
									{#if col.tree}
										<FileTree
											columnId={col.id}
											node={col.tree}
											depth={0}
											expandedPaths={col.expandedPaths}
											selectedPath={col.selectedPath}
											onSelect={(path, isDir) => handleFileSelect(path, col.id, isDir)}
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
	/* 滚动条样式由全局 `.wf-scroll-overlay` 提供，不在本组件本地覆盖。 */
</style>
