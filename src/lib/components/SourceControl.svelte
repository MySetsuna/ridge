<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { get } from 'svelte/store';
  import {
    ChevronRight,
    ChevronDown,
    GitBranch,
    GitCommit,
    GitPullRequestArrow,
    RefreshCw,
    Plus,
    Minus,
    Undo2,
    FileText,
    RotateCw,
    ArrowDown,
    ArrowUp,
    X,
    MoreHorizontal,
    Check,
  } from 'lucide-svelte';
  import { Splitpanes, Pane as SPane } from 'svelte-splitpanes';
  import {
    paneCwdStore,
    workspacesList,
    activeWorkspaceId,
    activePaneId,
    collapseCwd,
  } from '$lib/stores/paneTree';

  interface CommitNode {
    hash: string;
    subject: string;
    author: string;
    date: string;
    parents: string[];
    branch?: string;
  }
  interface DiffFile {
    path: string;
    additions: number;
    deletions: number;
    status: string;
  }
  interface GitRepoInfo {
    is_git_repo: boolean;
    commits: CommitNode[];
    branches: string[];
    current_branch: string | null;
    diff: {
      files: DiffFile[];
      total_additions: number;
      total_deletions: number;
      is_git_repo: boolean;
    };
  }
  interface ScmFile {
    path: string;
    status: string;
    group: string;
  }
  interface ScmRepoStatus {
    repo_root: string;
    current_branch: string | null;
    ahead: number;
    behind: number;
    staged: ScmFile[];
    changes: ScmFile[];
    untracked: ScmFile[];
  }

  // ─── Repo discovery (BFS dedupe of all pane cwds → git repo roots) ─────────
  // 扫描策略（性能优化）：
  //   - 仅在 cwd 集合真正变化时扫描（签名对比），不做周期轮询；
  //   - 前端 debounce 280 ms：cwd 连续变化（如启动多个终端）时合并为一次扫描；
  //   - 扫描放到空闲态（requestIdleCallback / setTimeout fallback），避免阻塞主线程；
  //   - 仓库根不变时跳过 find_git_repo_root 的整轮往返，只刷新 status。
  let repoRoots: string[] = $state([]);
  let statuses: Record<string, ScmRepoStatus> = $state({});
  let discoveryLoading = $state(false);
  let lastCwdSignature = '';
  let lastRepoSignature = '';
  let debounceHandle: ReturnType<typeof setTimeout> | undefined;
  let inFlight: Promise<void> | null = null;

  function schedule(run: () => Promise<void>, delayMs = 280): void {
    if (debounceHandle !== undefined) clearTimeout(debounceHandle);
    debounceHandle = setTimeout(() => {
      debounceHandle = undefined;
      const exec = () => {
        if (inFlight) return; // drop if already running
        inFlight = run().finally(() => {
          inFlight = null;
        });
      };
      const idle = (globalThis as unknown as { requestIdleCallback?: (cb: () => void) => number })
        .requestIdleCallback;
      if (typeof idle === 'function') idle(exec);
      else exec();
    }, delayMs);
  }

  async function discoverRepos(force = false): Promise<void> {
    if (!isTauri()) return;
    const cwds = get(paneCwdStore);
    const uniqueCwds = Array.from(new Set(Object.values(cwds).filter(Boolean))).sort();
    const sig = uniqueCwds.join('|');
    if (!force && sig === lastCwdSignature && repoRoots.length > 0) return;
    lastCwdSignature = sig;

    discoveryLoading = true;
    try {
      // 按用户要求：只向下扫描当前 cwd 的子目录找 .git，不再向上找。
      // 这意味着当前 cwd 就是仓库根 / 或它的父目录集 —— 子仓库都会被发现；
      // 若用户身处 `repo/src` 这样的深子目录，则不会再把 `repo` 识别成仓库
      // （这是用户明确要求的语义：`git仓库检索不需要向上层文件夹查找，只需要向下`）。
      const found = new Map<string, number>();
      await Promise.all(
        uniqueCwds.map(async (cwd) => {
          try {
            const roots = await invoke<string[]>('find_git_repos_below', { path: cwd, maxDepth: 4 });
            for (const r of roots) {
              found.set(r, (found.get(r) ?? 0) + 1);
            }
          } catch {
            /* ignore */
          }
        })
      );
      const nextRoots = Array.from(found.entries())
        .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
        .map(([root]) => root);

      const nextSig = nextRoots.join('|');
      const rootsChanged = nextSig !== lastRepoSignature;
      lastRepoSignature = nextSig;
      if (rootsChanged) {
        repoRoots = nextRoots;
        // Drop stale statuses for repos no longer present
        const keep: Record<string, ScmRepoStatus> = {};
        for (const r of nextRoots) if (statuses[r]) keep[r] = statuses[r];
        statuses = keep;
      }

      if (selectedRepo && !nextRoots.includes(selectedRepo)) {
        selectedRepo = nextRoots[0] ?? '';
      } else if (!selectedRepo && nextRoots.length > 0) {
        selectedRepo = nextRoots[0];
      }

      await Promise.all(nextRoots.map((root) => refreshStatus(root)));
      if (rootsChanged && selectedRepo) await loadGraph(selectedRepo);
    } finally {
      discoveryLoading = false;
    }
  }

  async function refreshStatus(root: string): Promise<void> {
    try {
      const s = await invoke<ScmRepoStatus>('get_scm_status', { repoRoot: root });
      statuses = { ...statuses, [root]: s };
    } catch (e) {
      console.error('get_scm_status failed', root, e);
    }
  }

  // ─── Selected repo for GitGraph section ────────────────────────────────────
  let selectedRepo = $state('');
  let graphInfo: GitRepoInfo | null = $state(null);
  let graphLoading = $state(false);
  let graphError: string | null = $state(null);

  async function loadGraph(root: string): Promise<void> {
    if (!isTauri() || !root) return;
    graphLoading = true;
    graphError = null;
    try {
      graphInfo = await invoke<GitRepoInfo>('get_git_info_with_cwd', { cwd: root });
    } catch (e) {
      graphError = String(e);
    } finally {
      graphLoading = false;
    }
  }

  // ─── Commit message + staging actions ──────────────────────────────────────
  let commitMessage: Record<string, string> = $state({});
  let committing = $state(false);

  async function stage(root: string, paths: string[]): Promise<void> {
    try {
      await invoke('git_stage', { repoRoot: root, paths });
      await refreshStatus(root);
    } catch (e) {
      alert(`Stage failed: ${e}`);
    }
  }
  async function unstage(root: string, paths: string[]): Promise<void> {
    try {
      await invoke('git_unstage', { repoRoot: root, paths });
      await refreshStatus(root);
    } catch (e) {
      alert(`Unstage failed: ${e}`);
    }
  }
  async function discard(root: string, paths: string[]): Promise<void> {
    if (paths.length === 0) return;
    if (!confirm(`丢弃 ${paths.length} 个文件的更改？此操作不可撤销。`)) return;
    try {
      await invoke('git_discard', { repoRoot: root, paths });
      await refreshStatus(root);
    } catch (e) {
      alert(`Discard failed: ${e}`);
    }
  }
  async function commit(root: string, amend = false): Promise<void> {
    const msg = (commitMessage[root] ?? '').trim();
    if (!msg) {
      alert('请输入提交信息');
      return;
    }
    committing = true;
    try {
      await invoke('git_commit', { repoRoot: root, message: msg, amend });
      commitMessage = { ...commitMessage, [root]: '' };
      await refreshStatus(root);
      if (root === selectedRepo) await loadGraph(root);
    } catch (e) {
      alert(`Commit failed: ${e}`);
    } finally {
      committing = false;
    }
  }

  // ─── 远端操作 + 分支切换（VSCode 风格）──────────────────────────────────
  interface BranchInfo {
    name: string;
    is_current: boolean;
    is_remote: boolean;
    upstream: string | null;
  }
  let branchLists: Record<string, BranchInfo[]> = $state({});
  let branchPickerOpen = $state<string>(''); // root whose picker is open
  let syncing = $state<string>(''); // root currently running a sync op

  async function loadBranches(root: string): Promise<void> {
    try {
      branchLists = {
        ...branchLists,
        [root]: await invoke<BranchInfo[]>('git_list_branches', { repoRoot: root }),
      };
    } catch (e) {
      console.error('list branches', e);
    }
  }
  async function openBranchPicker(root: string): Promise<void> {
    if (branchPickerOpen === root) {
      branchPickerOpen = '';
      return;
    }
    branchPickerOpen = root;
    await loadBranches(root);
  }
  async function switchBranch(root: string, branch: string): Promise<void> {
    branchPickerOpen = '';
    try {
      await invoke('git_checkout', { repoRoot: root, branch, create: false });
      await refreshStatus(root);
      await loadBranches(root);
      if (root === selectedRepo) await loadGraph(root);
    } catch (e) {
      alert(`切换分支失败: ${e}`);
    }
  }
  async function createBranch(root: string): Promise<void> {
    const name = prompt('新分支名称');
    if (!name || !name.trim()) return;
    branchPickerOpen = '';
    try {
      await invoke('git_checkout', { repoRoot: root, branch: name.trim(), create: true });
      await refreshStatus(root);
      await loadBranches(root);
    } catch (e) {
      alert(`创建分支失败: ${e}`);
    }
  }
  async function runSync(root: string, op: 'fetch' | 'pull' | 'push' | 'sync'): Promise<void> {
    if (syncing) return;
    syncing = root;
    try {
      const status = statuses[root];
      if (op === 'push' && status?.current_branch && !hasUpstream(root, status.current_branch)) {
        await invoke('git_push', { repoRoot: root, setUpstream: true });
      } else if (op === 'sync') {
        await invoke('git_sync', { repoRoot: root });
      } else {
        await invoke(`git_${op}`, { repoRoot: root });
      }
      await refreshStatus(root);
      await loadBranches(root);
      if (root === selectedRepo) await loadGraph(root);
    } catch (e) {
      alert(`${op} 失败: ${e}`);
    } finally {
      syncing = '';
    }
  }
  function hasUpstream(root: string, branchName: string): boolean {
    const list = branchLists[root] ?? [];
    const b = list.find((x) => x.name === branchName);
    return !!b?.upstream;
  }

  // ─── 差异预览（VSCode 风格：点击文件行打开 diff 弹窗）────────────────
  let diffOpen = $state(false);
  let diffTitle = $state('');
  let diffContent = $state('');
  let diffLoading = $state(false);
  async function showDiff(root: string, path: string, cached: boolean): Promise<void> {
    diffOpen = true;
    diffTitle = `${cached ? '[已暂存] ' : ''}${path}`;
    diffContent = '';
    diffLoading = true;
    try {
      diffContent = await invoke<string>('git_diff_file', { repoRoot: root, path, cached });
      if (!diffContent.trim()) diffContent = '(无差异或为新增文件；内容未进入 git 索引)';
    } catch (e) {
      diffContent = String(e);
    } finally {
      diffLoading = false;
    }
  }

  // 每次扫描完成后，同步加载各仓库的分支信息（供 header 显示 upstream 状态）。
  $effect(() => {
    for (const root of repoRoots) {
      if (!branchLists[root]) void loadBranches(root);
    }
  });

  function diffLineClass(line: string): string {
    if (line.startsWith('+++') || line.startsWith('---')) return 'text-[var(--wf-fg-muted)]';
    if (line.startsWith('+')) return 'text-green-400 bg-green-500/10';
    if (line.startsWith('-')) return 'text-red-400 bg-red-500/10';
    if (line.startsWith('@@')) return 'text-blue-400 bg-blue-500/10';
    return 'text-[var(--wf-fg)]';
  }

  // ─── Status label / color ──────────────────────────────────────────────────
  function statusColor(s: string): string {
    switch (s) {
      case 'M': return 'text-yellow-400';
      case 'A': return 'text-green-400';
      case 'D': return 'text-red-400';
      case 'R': return 'text-purple-400';
      case 'C': return 'text-blue-400';
      case '?': return 'text-gray-400';
      case 'U': return 'text-orange-400';
      default: return 'text-[var(--wf-fg-muted)]';
    }
  }
  function statusLabel(s: string): string {
    switch (s) {
      case 'M': return 'M';
      case 'A': return 'A';
      case 'D': return 'D';
      case 'R': return 'R';
      case 'C': return 'C';
      case '?': return 'U';
      case 'U': return '!';
      default: return s;
    }
  }
  function basename(p: string): string {
    return p.split(/[/\\]/).filter(Boolean).pop() || p;
  }
  function dirname(p: string): string {
    const parts = p.split(/[/\\]/).filter(Boolean);
    if (parts.length <= 1) return '';
    return parts.slice(0, -1).join('/');
  }
  function repoName(root: string): string {
    return basename(root);
  }
  function formatDate(ts: string): string {
    const d = new Date(parseInt(ts) * 1000);
    return d.toLocaleDateString('zh-CN', {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  }

  // ─── Collapsed state for per-repo groups ───────────────────────────────────
  let collapsedGroup = $state(new Set<string>()); // `${root}:${group}`
  function toggleGroup(root: string, group: string) {
    const key = `${root}:${group}`;
    const next = new Set(collapsedGroup);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    collapsedGroup = next;
  }
  function isCollapsed(root: string, group: string): boolean {
    return collapsedGroup.has(`${root}:${group}`);
  }

  // ─── 事件驱动：仅在 cwd 集合变化时扫描 git 仓库，不再做周期轮询 ─────────────
  // 节奏：
  //   - paneCwdStore 变动 → debounced discoverRepos（280 ms 合并）；
  //   - activeWorkspaceId 切换 → 立即重扫描（可能切到不同 cwd 集合）；
  //   - 手动刷新按钮 → 强制重扫 + 刷新所有仓库的 status；
  //   - 写操作（stage/unstage/discard/commit）已原地调用 refreshStatus，无需轮询兜底。
  onMount(() => {
    schedule(() => discoverRepos(), 0);
    const unsub1 = paneCwdStore.subscribe(() => schedule(() => discoverRepos()));
    const unsub2 = activeWorkspaceId.subscribe(() => schedule(() => discoverRepos(true), 0));
    return () => {
      unsub1();
      unsub2();
    };
  });

  onDestroy(() => {
    if (debounceHandle !== undefined) clearTimeout(debounceHandle);
  });

  async function manualRefresh(): Promise<void> {
    if (inFlight) return;
    await discoverRepos(true);
    await Promise.all(repoRoots.map((root) => refreshStatus(root)));
    if (selectedRepo) await loadGraph(selectedRepo);
  }

  // When selectedRepo changes, reload graph
  $effect(() => {
    if (selectedRepo) void loadGraph(selectedRepo);
  });
</script>

<div class="scm-root flex flex-col h-full min-h-0 wf-git-graph">
  <Splitpanes horizontal={true} theme="" class="wf-split flex-1 min-h-0">
    <!-- ═══ Top: Changes section ═══ -->
    <SPane size={50} minSize={20}>
      <div class="flex flex-col h-full min-h-0">
        <div
          class="px-3 h-9 shrink-0 flex items-center justify-between border-b border-[var(--wf-border)] bg-[var(--wf-surface)]/40"
        >
          <span class="text-[11px] font-semibold uppercase tracking-wider text-[var(--wf-fg-muted)]">
            更改
          </span>
          <div class="flex items-center gap-1">
            <span class="text-[10px] text-[var(--wf-fg-muted)]">{repoRoots.length} 仓库</span>
            <button
              type="button"
              class="flex h-6 w-6 items-center justify-center rounded text-[var(--wf-fg-muted)] hover:text-[var(--wf-fg)] hover:bg-[var(--wf-surface)]"
              title="刷新"
              onclick={() => void manualRefresh()}
            >
              <RefreshCw class="h-3 w-3 {discoveryLoading ? 'animate-spin' : ''}" />
            </button>
          </div>
        </div>

        <div class="flex-1 min-h-0 overflow-y-auto wf-scroll">
          {#if repoRoots.length === 0}
            <div class="p-4 text-[12px] text-[var(--wf-fg-muted)] text-center">
              {discoveryLoading ? '扫描中…' : '未在任意终端的 cwd 中检测到 Git 仓库。'}
            </div>
          {:else}
            {#each repoRoots as root (root)}
              {@const s = statuses[root]}
              <div class="scm-repo border-b border-[var(--wf-border)]/60 last:border-b-0 relative">
                <!-- Repo header（VSCode 风格）：仓库名 + 分支 picker + 同步/拉取/推送 -->
                <div class="px-3 py-1.5 bg-[var(--wf-surface)]/60 flex items-center gap-1.5 select-none">
                  <span class="text-[11px] font-semibold truncate flex-1 min-w-0" title={root}>
                    {repoName(root)}
                  </span>

                  <!-- 分支 picker 入口 -->
                  <button
                    type="button"
                    class="flex items-center gap-1 h-6 px-1.5 rounded text-[10px] bg-[var(--wf-accent)]/15 text-[var(--wf-accent)] hover:bg-[var(--wf-accent)]/25 transition-colors max-w-[140px]"
                    onclick={() => void openBranchPicker(root)}
                    title={s?.current_branch ? `当前分支：${s.current_branch}（点击切换）` : '切换分支'}
                  >
                    <GitBranch class="h-3 w-3 shrink-0" />
                    <span class="truncate">{s?.current_branch ?? '(detached)'}</span>
                  </button>

                  <!-- 上/下箭头显示 ahead/behind；点击触发 sync -->
                  {#if s && (s.ahead > 0 || s.behind > 0)}
                    <button
                      type="button"
                      class="flex items-center gap-0.5 h-6 px-1.5 rounded text-[10px] border border-[var(--wf-border)] text-[var(--wf-fg-muted)] hover:text-[var(--wf-fg)] hover:bg-[var(--wf-surface)] transition-colors"
                      onclick={() => void runSync(root, 'sync')}
                      disabled={syncing === root}
                      title="同步（fetch + pull + push）"
                    >
                      {#if s.behind > 0}<ArrowDown class="h-3 w-3" /><span>{s.behind}</span>{/if}
                      {#if s.ahead > 0}<ArrowUp class="h-3 w-3" /><span>{s.ahead}</span>{/if}
                    </button>
                  {/if}

                  <!-- 单独 Fetch / Pull / Push 按钮（VSCode overflow 菜单里的快捷替代）-->
                  <button
                    type="button"
                    class="flex h-6 w-6 items-center justify-center rounded text-[var(--wf-fg-muted)] hover:text-[var(--wf-fg)] hover:bg-[var(--wf-surface)]"
                    onclick={() => void runSync(root, 'fetch')}
                    disabled={syncing === root}
                    title="Fetch（git fetch --all --prune）"
                  >
                    <RotateCw class="h-3 w-3 {syncing === root ? 'animate-spin' : ''}" />
                  </button>
                  <button
                    type="button"
                    class="flex h-6 w-6 items-center justify-center rounded text-[var(--wf-fg-muted)] hover:text-[var(--wf-fg)] hover:bg-[var(--wf-surface)]"
                    onclick={() => void runSync(root, 'pull')}
                    disabled={syncing === root}
                    title="Pull（git pull --ff-only）"
                  >
                    <ArrowDown class="h-3 w-3" />
                  </button>
                  <button
                    type="button"
                    class="flex h-6 w-6 items-center justify-center rounded text-[var(--wf-fg-muted)] hover:text-[var(--wf-fg)] hover:bg-[var(--wf-surface)]"
                    onclick={() => void runSync(root, 'push')}
                    disabled={syncing === root}
                    title="Push（无 upstream 时自动 -u origin HEAD）"
                  >
                    <ArrowUp class="h-3 w-3" />
                  </button>
                </div>

                <!-- 分支 picker 下拉（绝对定位，覆盖头部下方；ESC/点击外部自行关闭留待下一轮）-->
                {#if branchPickerOpen === root}
                  {@const blist = branchLists[root] ?? []}
                  <div class="absolute left-3 right-3 top-[34px] z-40 bg-[var(--wf-bg)] border border-[var(--wf-border)] rounded shadow-lg max-h-[260px] overflow-y-auto">
                    <button
                      type="button"
                      class="w-full flex items-center gap-1.5 px-3 h-7 text-[11px] text-[var(--wf-accent)] hover:bg-[var(--wf-surface)] border-b border-[var(--wf-border)]/60 transition-colors"
                      onclick={() => void createBranch(root)}
                    >
                      <Plus class="h-3 w-3" /> 创建新分支…
                    </button>
                    {#each blist as b (b.name)}
                      <button
                        type="button"
                        class="group w-full flex items-center gap-1.5 px-3 h-7 text-[11px] text-[var(--wf-fg)] hover:bg-[var(--wf-surface)] transition-colors"
                        onclick={() => void switchBranch(root, b.name)}
                      >
                        {#if b.is_current}
                          <Check class="h-3 w-3 text-[var(--wf-accent)]" />
                        {:else}
                          <span class="w-3"></span>
                        {/if}
                        <GitBranch class="h-3 w-3 shrink-0 {b.is_remote ? 'text-blue-400/70' : 'text-[var(--wf-fg-muted)]'}" />
                        <span class="truncate flex-1 text-left">{b.name}</span>
                        {#if b.upstream}
                          <span class="text-[9px] text-[var(--wf-fg-muted)]/70 truncate">→ {b.upstream}</span>
                        {/if}
                      </button>
                    {/each}
                  </div>
                {/if}

                {#if s}
                  {@const totalChanges = s.staged.length + s.changes.length + s.untracked.length}

                  <!-- Commit box -->
                  {#if totalChanges > 0 || s.staged.length > 0}
                    <div class="px-3 py-2 flex flex-col gap-1.5 border-b border-[var(--wf-border)]/40">
                      <input
                        type="text"
                        class="w-full text-[12px] px-2 py-1 rounded bg-[var(--wf-bg)] border border-[var(--wf-border)] text-[var(--wf-fg)] focus:outline-none focus:border-[var(--wf-accent)]/60"
                        placeholder="消息（仅提交已暂存的更改）"
                        bind:value={commitMessage[root]}
                      />
                      <div class="flex items-center gap-1.5">
                        <button
                          type="button"
                          class="flex-1 flex items-center justify-center gap-1 px-2 py-1 rounded text-[11px] bg-[var(--wf-accent)]/15 text-[var(--wf-accent)] border border-[var(--wf-accent)]/30 hover:bg-[var(--wf-accent)]/25 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
                          onclick={() => commit(root, false)}
                          disabled={committing || s.staged.length === 0}
                          title={s.staged.length === 0 ? '请先暂存文件' : '提交已暂存的更改'}
                        >
                          <GitCommit class="h-3 w-3" /> 提交 {s.staged.length}
                        </button>
                        <button
                          type="button"
                          class="px-2 py-1 rounded text-[10px] border border-[var(--wf-border)] text-[var(--wf-fg-muted)] hover:text-[var(--wf-fg)] hover:bg-[var(--wf-surface)] disabled:opacity-40"
                          onclick={() => commit(root, true)}
                          disabled={committing || s.staged.length === 0}
                          title="修改最近一次提交（git commit --amend）"
                        >
                          Amend
                        </button>
                        {#if s.changes.length + s.untracked.length > 0}
                          <button
                            type="button"
                            class="px-2 py-1 rounded text-[11px] border border-[var(--wf-border)] text-[var(--wf-fg-muted)] hover:text-[var(--wf-fg)] hover:bg-[var(--wf-surface)]"
                            onclick={() =>
                              stage(
                                root,
                                [...s.changes, ...s.untracked].map((f) => f.path)
                              )}
                            title="暂存全部"
                          >
                            <Plus class="h-3 w-3" />
                          </button>
                        {/if}
                      </div>
                    </div>
                  {/if}

                  <!-- Staged group -->
                  {#if s.staged.length > 0}
                    <div class="group/grp scm-group">
                      <div class="w-full flex items-center gap-1 h-6 px-3 text-[10px] font-semibold uppercase tracking-wider text-[var(--wf-fg-muted)] hover:bg-[var(--wf-surface)]/50 transition-colors">
                        <button type="button" class="flex items-center gap-1 flex-1 text-left" onclick={() => toggleGroup(root, 'staged')}>
                          {#if isCollapsed(root, 'staged')}
                            <ChevronRight class="h-3 w-3" />
                          {:else}
                            <ChevronDown class="h-3 w-3" />
                          {/if}
                          <span class="flex-1">已暂存</span>
                        </button>
                        <button
                          type="button"
                          class="flex h-5 w-5 items-center justify-center rounded opacity-0 group-hover/grp:opacity-100 hover:bg-[var(--wf-surface)] hover:text-[var(--wf-fg)] transition-all"
                          title="撤销暂存全部"
                          onclick={() => unstage(root, s.staged.map((f) => f.path))}
                        >
                          <Minus class="h-3 w-3" />
                        </button>
                        <span class="text-[var(--wf-fg)]">{s.staged.length}</span>
                      </div>
                      {#if !isCollapsed(root, 'staged')}
                        {#each s.staged as f (f.path)}
                          <div
                            class="group flex items-center gap-1.5 h-6 pl-6 pr-3 text-[11px] hover:bg-[var(--wf-surface)]/50 transition-colors cursor-pointer"
                            title="{f.path}（点击查看差异）"
                            role="button"
                            tabindex="0"
                            onclick={() => void showDiff(root, f.path, true)}
                            onkeydown={(e) => e.key === 'Enter' && showDiff(root, f.path, true)}
                          >
                            <FileText class="h-3 w-3 shrink-0 text-[var(--wf-fg-muted)]" />
                            <span class="truncate text-[var(--wf-fg)]">{basename(f.path)}</span>
                            {#if dirname(f.path)}
                              <span class="text-[10px] text-[var(--wf-fg-muted)] truncate">
                                {dirname(f.path)}
                              </span>
                            {/if}
                            <span class="ml-auto flex items-center gap-0.5 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity">
                              <button
                                type="button"
                                class="flex h-5 w-5 items-center justify-center rounded hover:bg-[var(--wf-surface)] text-[var(--wf-fg-muted)] hover:text-[var(--wf-fg)]"
                                title="撤销暂存"
                                onclick={(e) => { e.stopPropagation(); void unstage(root, [f.path]); }}
                              >
                                <Minus class="h-3 w-3" />
                              </button>
                            </span>
                            <span class="shrink-0 font-mono text-[10px] w-3 text-right {statusColor(f.status)}">
                              {statusLabel(f.status)}
                            </span>
                          </div>
                        {/each}
                      {/if}
                    </div>
                  {/if}

                  <!-- Changes group -->
                  {#if s.changes.length > 0}
                    <div class="group/grp scm-group">
                      <div class="w-full flex items-center gap-1 h-6 px-3 text-[10px] font-semibold uppercase tracking-wider text-[var(--wf-fg-muted)] hover:bg-[var(--wf-surface)]/50 transition-colors">
                        <button type="button" class="flex items-center gap-1 flex-1 text-left" onclick={() => toggleGroup(root, 'changes')}>
                          {#if isCollapsed(root, 'changes')}
                            <ChevronRight class="h-3 w-3" />
                          {:else}
                            <ChevronDown class="h-3 w-3" />
                          {/if}
                          <span class="flex-1">更改</span>
                        </button>
                        <button
                          type="button"
                          class="flex h-5 w-5 items-center justify-center rounded opacity-0 group-hover/grp:opacity-100 hover:bg-[var(--wf-surface)] hover:text-red-400 transition-all"
                          title="丢弃全部未暂存更改"
                          onclick={() => discard(root, s.changes.map((f) => f.path))}
                        >
                          <Undo2 class="h-3 w-3" />
                        </button>
                        <button
                          type="button"
                          class="flex h-5 w-5 items-center justify-center rounded opacity-0 group-hover/grp:opacity-100 hover:bg-[var(--wf-surface)] hover:text-[var(--wf-fg)] transition-all"
                          title="暂存全部"
                          onclick={() => stage(root, s.changes.map((f) => f.path))}
                        >
                          <Plus class="h-3 w-3" />
                        </button>
                        <span class="text-[var(--wf-fg)]">{s.changes.length}</span>
                      </div>
                      {#if !isCollapsed(root, 'changes')}
                        {#each s.changes as f (f.path)}
                          <div
                            class="group flex items-center gap-1.5 h-6 pl-6 pr-3 text-[11px] hover:bg-[var(--wf-surface)]/50 transition-colors cursor-pointer"
                            title="{f.path}（点击查看差异）"
                            role="button"
                            tabindex="0"
                            onclick={() => void showDiff(root, f.path, false)}
                            onkeydown={(e) => e.key === 'Enter' && showDiff(root, f.path, false)}
                          >
                            <FileText class="h-3 w-3 shrink-0 text-[var(--wf-fg-muted)]" />
                            <span class="truncate text-[var(--wf-fg)]">{basename(f.path)}</span>
                            {#if dirname(f.path)}
                              <span class="text-[10px] text-[var(--wf-fg-muted)] truncate">
                                {dirname(f.path)}
                              </span>
                            {/if}
                            <span class="ml-auto flex items-center gap-0.5 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity">
                              <button
                                type="button"
                                class="flex h-5 w-5 items-center justify-center rounded hover:bg-[var(--wf-surface)] text-[var(--wf-fg-muted)] hover:text-[var(--wf-fg)]"
                                title="丢弃更改"
                                onclick={(e) => { e.stopPropagation(); void discard(root, [f.path]); }}
                              >
                                <Undo2 class="h-3 w-3" />
                              </button>
                              <button
                                type="button"
                                class="flex h-5 w-5 items-center justify-center rounded hover:bg-[var(--wf-surface)] text-[var(--wf-fg-muted)] hover:text-[var(--wf-fg)]"
                                title="暂存更改"
                                onclick={(e) => { e.stopPropagation(); void stage(root, [f.path]); }}
                              >
                                <Plus class="h-3 w-3" />
                              </button>
                            </span>
                            <span class="shrink-0 font-mono text-[10px] w-3 text-right {statusColor(f.status)}">
                              {statusLabel(f.status)}
                            </span>
                          </div>
                        {/each}
                      {/if}
                    </div>
                  {/if}

                  <!-- Untracked group -->
                  {#if s.untracked.length > 0}
                    <div class="scm-group">
                      <button
                        type="button"
                        class="w-full flex items-center gap-1 h-6 px-3 text-[10px] font-semibold uppercase tracking-wider text-[var(--wf-fg-muted)] hover:bg-[var(--wf-surface)]/50 transition-colors"
                        onclick={() => toggleGroup(root, 'untracked')}
                      >
                        {#if isCollapsed(root, 'untracked')}
                          <ChevronRight class="h-3 w-3" />
                        {:else}
                          <ChevronDown class="h-3 w-3" />
                        {/if}
                        <span class="flex-1 text-left">未跟踪</span>
                        <span class="text-[var(--wf-fg)]">{s.untracked.length}</span>
                      </button>
                      {#if !isCollapsed(root, 'untracked')}
                        {#each s.untracked as f (f.path)}
                          <div
                            class="group flex items-center gap-1.5 h-6 pl-6 pr-3 text-[11px] hover:bg-[var(--wf-surface)]/50 transition-colors"
                            title={f.path}
                          >
                            <FileText class="h-3 w-3 shrink-0 text-[var(--wf-fg-muted)]" />
                            <span class="truncate text-[var(--wf-fg)]">{basename(f.path)}</span>
                            {#if dirname(f.path)}
                              <span class="text-[10px] text-[var(--wf-fg-muted)] truncate">
                                {dirname(f.path)}
                              </span>
                            {/if}
                            <span class="ml-auto flex items-center gap-0.5 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity">
                              <button
                                type="button"
                                class="flex h-5 w-5 items-center justify-center rounded hover:bg-[var(--wf-surface)] text-[var(--wf-fg-muted)] hover:text-[var(--wf-fg)]"
                                title="暂存"
                                onclick={() => stage(root, [f.path])}
                              >
                                <Plus class="h-3 w-3" />
                              </button>
                            </span>
                            <span class="shrink-0 font-mono text-[10px] w-3 text-right {statusColor(f.status)}">
                              {statusLabel(f.status)}
                            </span>
                          </div>
                        {/each}
                      {/if}
                    </div>
                  {/if}

                  {#if totalChanges === 0}
                    <div class="px-3 py-2 text-[11px] text-[var(--wf-fg-muted)]">
                      工作区干净
                    </div>
                  {/if}
                {:else}
                  <div class="px-3 py-2 text-[11px] text-[var(--wf-fg-muted)]">加载中…</div>
                {/if}
              </div>
            {/each}
          {/if}
        </div>
      </div>
    </SPane>

    <!-- ═══ Bottom: Git Graph section ═══ -->
    <SPane size={50} minSize={20}>
      <div class="flex flex-col h-full min-h-0">
        <div
          class="px-3 h-9 shrink-0 flex items-center justify-between gap-2 border-b border-[var(--wf-border)] bg-[var(--wf-surface)]/40"
        >
          <span class="text-[11px] font-semibold uppercase tracking-wider text-[var(--wf-fg-muted)] shrink-0">
            图谱
          </span>
          {#if repoRoots.length > 0}
            <select
              class="flex-1 min-w-0 text-[11px] px-1.5 py-0.5 rounded bg-[var(--wf-bg)] border border-[var(--wf-border)] text-[var(--wf-fg)] focus:outline-none focus:border-[var(--wf-accent)]/60"
              bind:value={selectedRepo}
              title={selectedRepo}
            >
              {#each repoRoots as root (root)}
                <option value={root}>{repoName(root)} — {collapseCwd(root)}</option>
              {/each}
            </select>
            <button
              type="button"
              class="flex h-6 w-6 shrink-0 items-center justify-center rounded text-[var(--wf-fg-muted)] hover:text-[var(--wf-fg)] hover:bg-[var(--wf-surface)]"
              title="刷新"
              onclick={() => selectedRepo && loadGraph(selectedRepo)}
            >
              <RefreshCw class="h-3 w-3 {graphLoading ? 'animate-spin' : ''}" />
            </button>
          {/if}
        </div>

        <div class="flex-1 min-h-0 overflow-y-auto wf-scroll">
          {#if !selectedRepo}
            <div class="p-4 text-[12px] text-[var(--wf-fg-muted)] text-center">
              无 Git 仓库可显示
            </div>
          {:else if graphLoading && !graphInfo}
            <div class="p-4 text-[12px] text-[var(--wf-fg-muted)] text-center">加载中…</div>
          {:else if graphError}
            <div class="p-3 m-2 rounded bg-red-500/10 border border-red-500/20 text-[11px] text-red-400">
              {graphError}
            </div>
          {:else if graphInfo && graphInfo.is_git_repo}
            {#each graphInfo.commits as c (c.hash)}
              <div class="px-3 py-1.5 border-b border-[var(--wf-border)]/30 hover:bg-[var(--wf-surface)]/40 transition-colors">
                <div class="flex items-center gap-2">
                  <span class="text-[10px] font-mono text-[var(--wf-accent)] shrink-0">
                    {c.hash.slice(0, 7)}
                  </span>
                  {#if c.branch}
                    <span class="text-[10px] px-1 py-0 rounded bg-green-500/15 text-green-400 shrink-0">
                      {c.branch}
                    </span>
                  {/if}
                </div>
                <div class="text-[12px] text-[var(--wf-fg)] mt-0.5 truncate">{c.subject}</div>
                <div class="text-[10px] text-[var(--wf-fg-muted)] mt-0.5 truncate">
                  {c.author} · {formatDate(c.date)}
                </div>
              </div>
            {/each}
          {/if}
        </div>
      </div>
    </SPane>
  </Splitpanes>
</div>

<!-- ═══ Diff modal（VSCode SCM 点击文件查看差异）═══ -->
{#if diffOpen}
  <div
    role="presentation"
    class="fixed inset-0 z-[9998] bg-black/60 flex items-center justify-center"
    onclick={() => (diffOpen = false)}
  >
    <div
      role="dialog"
      aria-modal="true"
      aria-label="文件差异"
      class="w-[min(960px,92vw)] h-[min(720px,80vh)] flex flex-col bg-[var(--wf-bg)] border border-[var(--wf-border)] rounded-lg shadow-xl overflow-hidden"
      onclick={(e) => e.stopPropagation()}
    >
      <div class="flex items-center gap-2 h-9 px-3 border-b border-[var(--wf-border)] bg-[var(--wf-surface)]/60">
        <FileText class="h-3.5 w-3.5 text-[var(--wf-accent)]" />
        <span class="text-[12px] font-mono truncate flex-1" title={diffTitle}>{diffTitle}</span>
        <button
          type="button"
          class="flex h-6 w-6 items-center justify-center rounded text-[var(--wf-fg-muted)] hover:text-[var(--wf-fg)] hover:bg-[var(--wf-surface)] transition-colors"
          title="关闭"
          onclick={() => (diffOpen = false)}
        >
          <X class="h-3.5 w-3.5" />
        </button>
      </div>
      <div class="flex-1 min-h-0 overflow-auto wf-scroll bg-[var(--wf-bg)]">
        {#if diffLoading}
          <div class="p-4 text-[12px] text-[var(--wf-fg-muted)]">加载中…</div>
        {:else}
          <pre class="p-3 text-[11px] font-mono leading-5 whitespace-pre">{#each diffContent.split('\n') as line, idx (idx)}<span class={diffLineClass(line)}>{line}
</span>{/each}</pre>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .scm-root :global(.splitpanes__splitter) {
    background: var(--wf-border);
    min-height: 1px;
    height: 1px;
    position: relative;
  }
  .scm-root :global(.splitpanes__splitter::before) {
    content: '';
    position: absolute;
    left: 0;
    right: 0;
    top: -3px;
    bottom: -3px;
  }
  .scm-root :global(.splitpanes__splitter:hover) {
    background: var(--wf-accent);
  }
</style>
