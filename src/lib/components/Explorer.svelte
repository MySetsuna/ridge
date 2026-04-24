<script lang="ts">
	import { ChevronRight, RefreshCw, FolderOpen, Save, Trash2, FolderInput } from 'lucide-svelte';
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
		openWorkspaceFromFile,
	} from '$lib/stores/paneTree';
	import { fileEditorStore } from '$lib/stores/fileEditor';
	import { get } from 'svelte/store';
	import FileTree from './FileTree.svelte';
	import SaveWorkspaceDialog from './SaveWorkspaceDialog.svelte';

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

	// 通过 input[type=file] 选 .wind 文件内容，读出路径后交给后端打开。
	// 浏览器端 File 对象不提供真实绝对路径；Tauri 2 会注入 `file.path` 扩展属性。
	let fileInput: HTMLInputElement | undefined = $state();
	function triggerOpen() {
		fileInput?.click();
	}
	async function handleFileSelected(e: Event) {
		const input = e.currentTarget as HTMLInputElement;
		const file = input.files?.[0];
		if (!file) return;
		// Tauri 2 webview attaches absolute path to File; fall back to text prompt otherwise.
		const p = (file as File & { path?: string }).path;
		let chosen = p;
		if (!chosen) {
			chosen = prompt('输入 .wind 文件绝对路径') ?? '';
		}
		input.value = ''; // allow re-selecting the same file later
		if (!chosen) return;
		try {
			await openWorkspaceFromFile(chosen);
		} catch (err) {
			alert(`打开失败: ${err}`);
		}
	}
</script>

<!-- 隐藏的文件选择器：仅用于触发 .wind 打开流程。 -->
<input
	bind:this={fileInput}
	type="file"
	accept=".wind,application/json"
	class="hidden"
	onchange={handleFileSelected}
/>

<SaveWorkspaceDialog
	bind:open={saveDialogOpen}
	defaultName={saveDefaultName}
	onConfirm={handleSaveConfirm}
	onCancel={() => (saveDialogOpen = false)}
/>

<div class="explorer flex h-full flex-col overflow-y-auto" data-testid="file-tree">
	<!-- 顶部工具栏：打开已保存工作区入口。 -->
	<div
		class="shrink-0 flex items-center justify-between h-8 px-2 gap-1 border-b border-[var(--wf-border)] bg-[var(--wf-surface)]/40"
	>
		<span class="text-[11px] font-semibold uppercase tracking-wider text-[var(--wf-fg-muted)]">
			资源管理器
		</span>
		<button
			type="button"
			class="flex items-center gap-1 px-1.5 h-6 rounded text-[10px] text-[var(--wf-fg-muted)] hover:text-[var(--wf-fg)] hover:bg-[var(--wf-surface)] transition-colors"
			title="从 .wind 文件打开已保存的工作区"
			onclick={triggerOpen}
		>
			<FolderInput class="h-3 w-3" /> 打开工作区
		</button>
	</div>
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
								class="group flex items-center h-7 px-2 gap-1 cursor-pointer select-none hover:bg-[var(--wf-surface)]/60 transition-colors"
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
