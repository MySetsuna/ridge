<script lang="ts">
  // src/lib/components/ClaudeCodePanel.svelte

  import { ChevronRight, ChevronDown, Bot, Trash2, Play, Settings, FolderOpen, GitBranch, FileText, FileDiff } from 'lucide-svelte';
  import {
    workspacesList,
    activeWorkspaceId,
    paneTreeStore,
    paneCwdStore,
    type PaneNode,
  } from '$lib/stores/paneTree';
  import {
    claudeHistoryStore,
    clearHistoryForPane,
    getHistoryForPane,
  } from '$lib/plugins/claudeHistory/store';
  import { openClaudeAgentLauncher } from './ClaudeAgentLauncher.svelte';
  import { settingsStore, setClaudeExtensionEnabled } from '$lib/stores/settings';
  import { overlayScroll } from '$lib/actions/overlayScroll';
  import { portal } from '$lib/actions/portal';
  import { popupStyleFor } from '$lib/utils/anchorRect';
  import { openDiffEditor } from './DiffEditorModal.svelte';
  import { isTauri, invoke } from '@tauri-apps/api/core';
  import { onMount, onDestroy } from 'svelte';
  import { get } from 'svelte/store';

  interface ClaudeHistoryEntry {
    display: string;
    timestamp: number;
    project: string;
    session_id?: string;
  }

  interface ScmFile {
    path: string;
    status: string;
    group: string;
  }

  interface ScmStatus {
    staged: ScmFile[];
    changes: ScmFile[];
    untracked: ScmFile[];
  }

  interface EntryChanges {
    loading: boolean;
    files: Array<{ path: string; status: string; cached: boolean }>;
    error: boolean;
  }

  // File-based history from ~/.claude/history.jsonl
  let fileHistory = $state<ClaudeHistoryEntry[]>([]);
  let fileHistoryLoading = $state(false);
  let fileHistoryCollapsed = $state(false);
  // T6：默认按项目分组。读取 localStorage 让用户上次的选择得以保留。
  const FILE_HISTORY_VIEW_KEY = 'ridge-claude-history-view';
  function loadHistoryView(): 'flat' | 'byProject' {
    if (typeof localStorage === 'undefined') return 'byProject';
    const raw = localStorage.getItem(FILE_HISTORY_VIEW_KEY);
    return raw === 'flat' ? 'flat' : 'byProject';
  }
  let fileHistoryView = $state<'flat' | 'byProject'>(loadHistoryView());
  $effect(() => {
    if (typeof localStorage === 'undefined') return;
    try {
      localStorage.setItem(FILE_HISTORY_VIEW_KEY, fileHistoryView);
    } catch {
      /* ignore */
    }
  });

  // Expanded entry (click to show git changes)
  let expandedHistoryKey = $state<string | null>(null);
  let entryChanges = $state(new Map<string, EntryChanges>());

  // Per-project collapse state in by-project view. Persisted so the user's
  // "show only my current project" shape survives across sessions.
  const COLLAPSED_PROJECTS_KEY = 'ridge-claude-history-collapsed-projects';
  let collapsedProjects = $state(loadCollapsedProjects());

  function loadCollapsedProjects(): Set<string> {
    if (typeof localStorage === 'undefined') return new Set();
    try {
      const raw = localStorage.getItem(COLLAPSED_PROJECTS_KEY);
      if (!raw) return new Set();
      const arr = JSON.parse(raw);
      return Array.isArray(arr) ? new Set(arr.filter((s) => typeof s === 'string')) : new Set();
    } catch {
      return new Set();
    }
  }

  function persistCollapsedProjects(s: Set<string>): void {
    if (typeof localStorage === 'undefined') return;
    try {
      localStorage.setItem(COLLAPSED_PROJECTS_KEY, JSON.stringify(Array.from(s)));
    } catch {
      /* ignore quota / privacy errors */
    }
  }

  function toggleProjectCollapsed(proj: string): void {
    const next = new Set(collapsedProjects);
    if (next.has(proj)) next.delete(proj);
    else next.add(proj);
    collapsedProjects = next;
    persistCollapsedProjects(next);
  }

  function historyKey(e: ClaudeHistoryEntry): string {
    return `${e.timestamp}:${e.session_id ?? e.project}`;
  }

  async function toggleHistoryEntry(entry: ClaudeHistoryEntry): Promise<void> {
    const key = historyKey(entry);
    if (expandedHistoryKey === key) {
      expandedHistoryKey = null;
      return;
    }
    expandedHistoryKey = key;
    if (entryChanges.has(key)) return;
    const next = new Map(entryChanges);
    next.set(key, { loading: true, files: [], error: false });
    entryChanges = next;
    try {
      const status = await invoke<ScmStatus>('get_scm_status', { repoRoot: entry.project });
      const files = [
        ...status.staged.map((f) => ({ path: f.path, status: f.status, cached: true })),
        ...status.changes.map((f) => ({ path: f.path, status: f.status, cached: false })),
      ];
      const done = new Map(entryChanges);
      done.set(key, { loading: false, files, error: false });
      entryChanges = done;
    } catch {
      const err = new Map(entryChanges);
      err.set(key, { loading: false, files: [], error: true });
      entryChanges = err;
    }
  }

  // Grouped view
  const fileHistoryByProject = $derived.by(() => {
    const map = new Map<string, { label: string; entries: ClaudeHistoryEntry[] }>();
    for (const entry of fileHistory) {
      const proj = entry.project.replace(/\\/g, '/');
      const label = proj.split('/').pop() ?? proj;
      if (!map.has(proj)) map.set(proj, { label, entries: [] });
      map.get(proj)!.entries.push(entry);
    }
    return Array.from(map.entries()).map(([proj, v]) => ({ proj, ...v }));
  });

  async function loadFileHistory(): Promise<void> {
    if (!isTauri()) return;
    fileHistoryLoading = true;
    try {
      // Pass empty array — no filtering, show all history across all projects
      fileHistory = await invoke<ClaudeHistoryEntry[]>('read_claude_history', {
        projectPaths: [],
        limit: 100,
      });
    } catch {
      // Silently ignore: ~/.claude/history.jsonl may not exist
    } finally {
      fileHistoryLoading = false;
    }
  }

  let refreshTimer: ReturnType<typeof setInterval> | undefined;
  onMount(() => {
    void loadFileHistory();
    refreshTimer = setInterval(() => { void loadFileHistory(); }, 30_000);
  });
  onDestroy(() => {
    if (refreshTimer !== undefined) clearInterval(refreshTimer);
  });

  $effect(() => {
    const _cwds = $paneCwdStore;
    void loadFileHistory();
  });

  let collapsedPanes = $state(new Set<string>());
  function togglePane(paneId: string): void {
    const next = new Set(collapsedPanes);
    if (next.has(paneId)) next.delete(paneId);
    else next.add(paneId);
    collapsedPanes = next;
  }

  let settingsOpen = $state(false);
  let settingsAnchor: HTMLElement | undefined = $state();

  $effect(() => {
    function onMouseDown(ev: MouseEvent) {
      if (!settingsOpen) return;
      const target = ev.target as Node | null;
      if (settingsAnchor && target && settingsAnchor.contains(target)) return;
      settingsOpen = false;
    }
    function onKey(ev: KeyboardEvent) {
      if (settingsOpen && ev.key === 'Escape') {
        ev.preventDefault();
        settingsOpen = false;
      }
    }
    document.addEventListener('mousedown', onMouseDown, true);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onMouseDown, true);
      document.removeEventListener('keydown', onKey);
    };
  });

  interface LeafEntry {
    paneId: string;
    agentState: 'idle' | 'busy' | 'launching' | undefined;
  }
  function flattenLeaves(node: PaneNode | null | undefined, out: LeafEntry[] = []): LeafEntry[] {
    if (!node) return out;
    if (node.type === 'leaf') {
      out.push({
        paneId: node.id,
        agentState: (node as { agent_state?: 'idle' | 'busy' | 'launching' }).agent_state,
      });
      return out;
    }
    for (const child of node.children) flattenLeaves(child, out);
    return out;
  }

  const flattened = $derived(flattenLeaves($paneTreeStore));
  const activeWs = $derived($workspacesList.find((w) => w.id === $activeWorkspaceId));

  function timestamp(at: number): string {
    const d = new Date(at);
    return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
  }

  function dateLabel(at: number): string {
    return new Date(at).toLocaleString('zh-CN', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' });
  }

  function preview(text: string, max = 60): string {
    const oneLine = text.replace(/\s+/g, ' ').trim();
    if (!oneLine) return '(空 prompt · REPL)';
    return oneLine.length > max ? `${oneLine.slice(0, max - 1)}…` : oneLine;
  }

  function shortCwd(cwd: string | undefined): string {
    if (!cwd) return '';
    const parts = cwd.split(/[\\/]/).filter(Boolean);
    if (parts.length <= 2) return cwd;
    return '…/' + parts.slice(-2).join('/');
  }

  function statusLabel(s: string): string {
    const map: Record<string, string> = { M: '改', A: '增', D: '删', R: '移', C: '复', U: '冲' };
    return map[s[0]?.toUpperCase() ?? ''] ?? s[0] ?? '?';
  }

  function statusColor(s: string): string {
    const c = s[0]?.toUpperCase() ?? '';
    if (c === 'M' || c === 'U') return 'text-amber-400';
    if (c === 'A') return 'text-emerald-400';
    if (c === 'D') return 'text-red-400';
    return 'text-[var(--rg-fg-muted)]';
  }

  $effect(() => {
    for (const leaf of flattened) {
      getHistoryForPane(leaf.paneId);
    }
  });

  function disableExtension(): void {
    settingsOpen = false;
    setClaudeExtensionEnabled(false);
  }
</script>

<!-- Header -->
<div
  data-tauri-drag-region
  class="px-3 h-11 items-center flex justify-between shrink-0 border-b border-[var(--rg-border)] text-xs font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)] relative"
>
  <span class="flex items-center gap-1.5">
    <Bot class="h-3.5 w-3.5 text-emerald-400" />
    Claude Code
  </span>
  <div class="flex items-center gap-0.5" bind:this={settingsAnchor}>
    <button
      type="button"
      class="flex h-7 w-7 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)] transition-colors"
      title="扩展设置"
      onclick={() => (settingsOpen = !settingsOpen)}
    >
      <Settings class="h-3.5 w-3.5" />
    </button>
  {#if settingsOpen && settingsAnchor}
    <div
      class="z-[9990] min-w-[220px] rounded-lg border border-[var(--rg-border)] bg-[var(--rg-bg-raised)] shadow-xl py-1 text-[12px]"
      style={popupStyleFor(settingsAnchor, 'bottom-end')}
      data-rg-portal-id="claude-code-panel-settings"
      use:portal={{ id: 'claude-code-panel-settings' }}
    >
      <div class="px-3 py-1 text-[10px] uppercase tracking-wider text-[var(--rg-fg-muted)]">
        扩展
      </div>
      <button
        type="button"
        class="w-full flex items-center gap-2 px-3 py-1.5 text-left text-[var(--rg-fg)] hover:bg-[var(--rg-surface)] transition-colors"
        onclick={disableExtension}
        title="关闭后左侧 rail 的 Bot 按钮、pane 标题的 Bot 按钮都会消失；可在工作区设置重新开启"
      >
        关闭 Claude Code 扩展
      </button>
    </div>
  {/if}
  </div>
</div>

<!-- Body -->
<div class="flex-1 min-h-0 flex flex-col" use:overlayScroll>
  {#if flattened.length === 0}
    <div class="p-4 text-[12px] text-[var(--rg-fg-muted)] text-center">
      当前工作区无 pane —— 打开终端后将在此显示。
    </div>
  {:else if activeWs}
    <!-- T6：移除工作区栏 —— 直接渲染 pane 列表，没有"工作区 N · 4 panes"sticky 头。 -->
    <div class="last:border-b-0">
      {#each flattened as leaf (leaf.paneId)}
        {@const cwd = $paneCwdStore[`${activeWs.id}:${leaf.paneId}`] ?? $paneCwdStore[leaf.paneId]}
          {@const entries = $claudeHistoryStore[leaf.paneId] ?? []}
          {@const collapsed = collapsedPanes.has(leaf.paneId)}
          <div class="border-t border-[var(--rg-border)]/30">
            <!-- Pane row -->
            <div class="px-3 py-1.5 flex items-center gap-1.5 hover:bg-[var(--rg-surface)]/40 transition-colors">
              <button
                type="button"
                class="flex h-5 w-5 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]"
                onclick={() => togglePane(leaf.paneId)}
                title={collapsed ? '展开历史' : '收起历史'}
              >
                {#if collapsed}
                  <ChevronRight class="h-3 w-3" />
                {:else}
                  <ChevronDown class="h-3 w-3" />
                {/if}
              </button>
              <span title={leaf.agentState ?? 'idle'} class="flex shrink-0">
                <Bot
                  class="h-3.5 w-3.5 {leaf.agentState === 'busy'
                    ? 'text-emerald-400 animate-pulse'
                    : leaf.agentState === 'launching'
                    ? 'text-amber-400'
                    : 'text-[var(--rg-fg-muted)]'}"
                />
              </span>
              <span class="flex-1 min-w-0 truncate text-[11px] text-[var(--rg-fg)]" title={cwd}>
                {leaf.paneId.slice(0, 6)}<span class="text-[var(--rg-fg-muted)] ml-1">{shortCwd(cwd)}</span>
              </span>
              <span class="text-[9px] text-[var(--rg-fg-muted)]/70 shrink-0">
                {entries.length}
              </span>
              <button
                type="button"
                class="flex h-5 w-5 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:text-emerald-300 hover:bg-emerald-500/10 transition-colors disabled:opacity-30 disabled:pointer-events-none"
                title={leaf.agentState === 'busy'
                  ? '此 pane 已有 agent 运行'
                  : '在此 pane 启动 Claude（Shift-Click 跳过 prompt 直接启动）'}
                disabled={leaf.agentState === 'busy' || !isTauri()}
                onclick={(e) => openClaudeAgentLauncher(leaf.paneId, e.shiftKey || e.altKey)}
              >
                <Play class="h-3 w-3" />
              </button>
            </div>

            <!-- History list (when expanded) -->
            {#if !collapsed}
              {#if entries.length === 0}
                <div class="px-9 pb-1.5 text-[10px] text-[var(--rg-fg-muted)]">
                  尚无历史 prompt。
                </div>
              {:else}
                {#each entries.slice().reverse() as entry (entry.at + ':' + entry.agentId)}
                  <button
                    type="button"
                    class="w-full flex items-start gap-2 pl-9 pr-3 py-1 text-left text-[11px] hover:bg-[var(--rg-surface)]/50 transition-colors"
                    title={entry.prompt || '(REPL — 无 prompt)'}
                    onclick={() => openClaudeAgentLauncher(leaf.paneId, false)}
                  >
                    <span class="shrink-0 font-mono text-[9px] text-[var(--rg-fg-muted)] w-8 text-right">
                      {timestamp(entry.at)}
                    </span>
                    <span class="truncate text-[var(--rg-fg)]">{preview(entry.prompt)}</span>
                  </button>
                {/each}
                <div class="pl-9 pr-3 py-1">
                  <button
                    type="button"
                    class="flex items-center gap-1 h-5 px-1.5 rounded text-[10px] text-[var(--rg-fg-muted)] hover:text-red-400 hover:bg-[var(--rg-surface)]/50 transition-colors"
                    onclick={() => clearHistoryForPane(leaf.paneId)}
                    title="清空此 pane 的 Claude 历史"
                  >
                    <Trash2 class="h-3 w-3" /> 清空
                  </button>
                </div>
              {/if}
            {/if}
          </div>
        {/each}
      </div>
    {/if}

  <!-- ── 命令行历史：来自 ~/.claude/history.jsonl ── -->
  {#if fileHistory.length > 0 || fileHistoryLoading}
    <div class="border-t border-[var(--rg-border)] mt-auto min-h-0 flex flex-col">
      <!-- Section header -->
      <div
        class="sticky top-0 z-10 w-full px-3 h-7 flex items-center gap-1.5 bg-[var(--rg-surface-2)]/92 backdrop-blur-md text-[10px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)] cursor-pointer select-none hover:bg-[var(--rg-surface)] transition-colors"
        role="button"
        tabindex="0"
        onclick={() => { fileHistoryCollapsed = !fileHistoryCollapsed; }}
        onkeydown={(e) => e.key === 'Enter' && (fileHistoryCollapsed = !fileHistoryCollapsed)}
        title="来自 ~/.claude/history.jsonl 的历史记录"
      >
        <ChevronRight class="h-3 w-3 transition-transform duration-150 {fileHistoryCollapsed ? '' : 'rotate-90'}" />
        <span class="flex-1">命令行历史</span>
        {#if fileHistoryLoading}
          <span class="text-[9px] opacity-50">…</span>
        {:else if fileHistoryView === 'byProject'}
          <span class="text-[var(--rg-fg)]">{fileHistoryByProject.length} 个项目</span>
        {:else}
          <span class="text-[var(--rg-fg)]">{fileHistory.length}</span>
        {/if}

        <!-- View toggle: flat / by-project -->
        {#if !fileHistoryCollapsed && fileHistory.length > 0}
          <button
            type="button"
            class="flex items-center gap-0.5 h-5 px-1.5 rounded text-[10px] border transition-colors
              {fileHistoryView === 'byProject'
                ? 'bg-[var(--rg-accent)]/20 border-[var(--rg-accent)]/40 text-[var(--rg-accent)]'
                : 'border-[var(--rg-border)] text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]'}"
            title={fileHistoryView === 'byProject' ? '切换为按时间排列' : '切换为按项目分组'}
            onclick={(e) => {
              e.stopPropagation();
              fileHistoryView = fileHistoryView === 'flat' ? 'byProject' : 'flat';
            }}
          >
            <FolderOpen class="h-2.5 w-2.5" />
          </button>
        {/if}

        <button
          type="button"
          class="flex h-5 w-5 items-center justify-center rounded hover:bg-[var(--rg-accent)]/20 hover:text-[var(--rg-fg)] transition-colors"
          title="刷新历史"
          onclick={(e) => { e.stopPropagation(); void loadFileHistory(); }}
        >
          <svg class="h-3 w-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/>
            <path d="M3 3v5h5"/>
          </svg>
        </button>
      </div>

      {#if !fileHistoryCollapsed}
        <div class="flex-1 min-h-0 overflow-y-auto" use:overlayScroll>
        {#if fileHistoryView === 'flat'}
          <!-- Flat view: all entries sorted by time -->
          {#each fileHistory as entry (historyKey(entry))}
            {@const key = historyKey(entry)}
            {@const isExpanded = expandedHistoryKey === key}
            {@const changes = entryChanges.get(key)}
            <div class="border-t border-[var(--rg-border)]/20">
              <button
                type="button"
                class="w-full flex items-start gap-2 px-3 py-1.5 text-left hover:bg-[var(--rg-surface)]/40 transition-colors group {isExpanded ? 'bg-[var(--rg-surface)]/30' : ''}"
                onclick={() => void toggleHistoryEntry(entry)}
              >
                <ChevronRight class="h-3 w-3 shrink-0 mt-0.5 text-[var(--rg-fg-muted)] transition-transform duration-150 {isExpanded ? 'rotate-90' : ''}" />
                <Bot class="h-3 w-3 shrink-0 mt-0.5 text-[var(--rg-fg-muted)]" />
                <div class="flex-1 min-w-0">
                  <div class="truncate text-[11px] text-[var(--rg-fg)]">{preview(entry.display)}</div>
                  <div class="text-[9px] text-[var(--rg-fg-muted)] flex items-center gap-1.5 mt-0.5">
                    <span class="font-mono">{dateLabel(entry.timestamp)}</span>
                    <span class="truncate opacity-70">{entry.project.replace(/\\/g,'/').split('/').pop()}</span>
                  </div>
                </div>
              </button>
              {#if isExpanded}
                <div class="pl-8 pr-3 pb-1.5">
                  {#if changes?.loading}
                    <div class="text-[10px] text-[var(--rg-fg-muted)] py-1">加载变更中…</div>
                  {:else if changes?.error}
                    <div class="text-[10px] text-[var(--rg-fg-muted)]/60 py-1">无法读取 git 状态</div>
                  {:else if changes && changes.files.length === 0}
                    <div class="text-[10px] text-[var(--rg-fg-muted)]/60 py-1">无未提交变更</div>
                  {:else if changes}
                    {#each changes.files as f (f.path)}
                      <button
                        type="button"
                        class="w-full flex items-center gap-1.5 py-0.5 text-left text-[11px] hover:bg-[var(--rg-accent)]/10 rounded px-1 transition-colors"
                        title="点击查看 diff：{f.path}"
                        onclick={() => openDiffEditor({ repoRoot: entry.project, path: f.path, cached: f.cached })}
                      >
                        <span class="shrink-0 text-[10px] font-mono w-4 text-center {statusColor(f.status)}">{statusLabel(f.status)}</span>
                        <span class="truncate text-[var(--rg-fg-muted)]">{f.path}</span>
                        <FileDiff class="h-2.5 w-2.5 shrink-0 text-[var(--rg-fg-muted)]/40 ml-auto" />
                      </button>
                    {/each}
                  {/if}
                </div>
              {/if}
            </div>
          {/each}
        {:else}
          <!-- By-project view: grouped by project directory -->
          {#each fileHistoryByProject as group (group.proj)}
            {@const isProjectCollapsed = collapsedProjects.has(group.proj)}
            <div class="border-t border-[var(--rg-border)]/40">
              <button
                type="button"
                class="w-full px-3 py-1 flex items-center gap-1.5 bg-[var(--rg-surface)]/30 hover:bg-[var(--rg-surface)]/50 text-[10px] text-[var(--rg-fg-muted)] font-semibold uppercase tracking-wider transition-colors text-left"
                title={isProjectCollapsed ? `展开 ${group.label}` : `折叠 ${group.label}`}
                onclick={() => toggleProjectCollapsed(group.proj)}
              >
                <ChevronRight
                  class="h-3 w-3 shrink-0 transition-transform duration-150 {isProjectCollapsed ? '' : 'rotate-90'}"
                />
                <FolderOpen class="h-3 w-3 shrink-0 text-[var(--rg-accent)]" />
                <span class="truncate" title={group.proj}>{group.label}</span>
                <span class="ml-auto text-[var(--rg-fg)]">{group.entries.length}</span>
              </button>
              {#if !isProjectCollapsed}
              {#each group.entries as entry (historyKey(entry))}
                {@const key = historyKey(entry)}
                {@const isExpanded = expandedHistoryKey === key}
                {@const changes = entryChanges.get(key)}
                <div class="border-t border-[var(--rg-border)]/10">
                  <button
                    type="button"
                    class="w-full flex items-start gap-2 pl-6 pr-3 py-1.5 text-left hover:bg-[var(--rg-surface)]/40 transition-colors {isExpanded ? 'bg-[var(--rg-surface)]/30' : ''}"
                    onclick={() => void toggleHistoryEntry(entry)}
                  >
                    <ChevronRight class="h-3 w-3 shrink-0 mt-0.5 text-[var(--rg-fg-muted)] transition-transform duration-150 {isExpanded ? 'rotate-90' : ''}" />
                    <div class="flex-1 min-w-0">
                      <div class="truncate text-[11px] text-[var(--rg-fg)]">{preview(entry.display)}</div>
                      <span class="font-mono text-[9px] text-[var(--rg-fg-muted)]">{dateLabel(entry.timestamp)}</span>
                    </div>
                  </button>
                  {#if isExpanded}
                    <div class="pl-10 pr-3 pb-1.5">
                      {#if changes?.loading}
                        <div class="text-[10px] text-[var(--rg-fg-muted)] py-1">加载变更中…</div>
                      {:else if changes?.error}
                        <div class="text-[10px] text-[var(--rg-fg-muted)]/60 py-1">无法读取 git 状态</div>
                      {:else if changes && changes.files.length === 0}
                        <div class="text-[10px] text-[var(--rg-fg-muted)]/60 py-1">无未提交变更</div>
                      {:else if changes}
                        {#each changes.files as f (f.path)}
                          <button
                            type="button"
                            class="w-full flex items-center gap-1.5 py-0.5 text-left text-[11px] hover:bg-[var(--rg-accent)]/10 rounded px-1 transition-colors"
                            title="点击查看 diff：{f.path}"
                            onclick={() => openDiffEditor({ repoRoot: entry.project, path: f.path, cached: f.cached })}
                          >
                            <span class="shrink-0 text-[10px] font-mono w-4 text-center {statusColor(f.status)}">{statusLabel(f.status)}</span>
                            <span class="truncate text-[var(--rg-fg-muted)]">{f.path}</span>
                            <FileDiff class="h-2.5 w-2.5 shrink-0 text-[var(--rg-fg-muted)]/40 ml-auto" />
                          </button>
                        {/each}
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
      {/if}
    </div>
  {/if}
  </div>
