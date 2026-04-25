<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
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
    Copy,
    Scissors,
    GitMerge,
    Tag,
  } from 'lucide-svelte';
  import { showContextMenu, type ContextMenuItem } from '$lib/stores/contextMenu';
  import { Splitpanes, Pane as SPane } from 'svelte-splitpanes';
  import {
    paneCwdStore,
    workspacesList,
    activeWorkspaceId,
    activePaneId,
    collapseCwd,
  } from '$lib/stores/paneTree';
  import { overlayScroll } from '$lib/actions/overlayScroll';
  import { invalidatePaneGitStatusForRepo } from '$lib/stores/paneGitStatus';
  import {
    scmCacheStore,
    getScmCache,
    setScmRepoRoots,
    setScmRepoStatus,
    shouldRefreshOnMount,
    setScmGraphInfo,
    shouldRefreshGraphOnMount,
    setScmSelectedCommit,
    getScmSelectedCommit,
    setScmSelectedRepo,
    type GitRepoInfo,
    type CommitNode,
    type DiffFile,
  } from '$lib/stores/scmCache';
  import { openDiffEditor } from './DiffEditorModal.svelte';
  import { alertDialog, confirmDialog, promptDialog } from './WindDialog.svelte';
  import GitGraph from './GitGraph.svelte';
  import { DEFAULT_DY as GRAPH_ROW_HEIGHT } from './gitGraphLayout';

  // Maximum ref pills shown inline before an overflow badge collapses the rest.
  // HEAD + one branch is the natural pair, so the HEAD exception adds +1 when
  // the first ref is `head:` (see splitRefs below).
  const MAX_VISIBLE_REFS = 2;

  /**
   * Split a commit's refs into [visible, hidden] sub-arrays.
   * If refs[0] === 'head:', the visible window grows by 1 so HEAD and its
   * branch name always appear together as a natural pair.
   */
  function splitRefs(refs: string[] | undefined): { visible: string[]; hidden: string[] } {
    if (!refs || refs.length === 0) return { visible: [], hidden: [] };
    const headOffset = refs[0] === 'head:' ? 1 : 0;
    const maxVisible = MAX_VISIBLE_REFS + headOffset;
    return { visible: refs.slice(0, maxVisible), hidden: refs.slice(maxVisible) };
  }

  // CommitNode / DiffFile / GitRepoInfo are imported from scmCache (round χ).
  interface ScmFile {
    path: string;
    status: string;
    group: string;
    /** Optional — backend `#[serde(default)]`. */
    additions?: number;
    deletions?: number;
  }
  interface ScmRepoStatus {
    repo_root: string;
    current_branch: string | null;
    ahead: number;
    behind: number;
    staged: ScmFile[];
    changes: ScmFile[];
    untracked: ScmFile[];
    /** Backend `#[serde(default)]` — optional so older snapshots stay valid. */
    has_upstream?: boolean;
  }

  // ─── Repo discovery (BFS dedupe of all pane cwds → git repo roots) ─────────
  // 扫描策略（性能优化）：
  //   - 仅在 cwd 集合真正变化时扫描（签名对比），不做周期轮询；
  //   - 前端 debounce 280 ms：cwd 连续变化（如启动多个终端）时合并为一次扫描；
  //   - 扫描放到空闲态（requestIdleCallback / setTimeout fallback），避免阻塞主线程；
  //   - 仓库根不变时跳过 find_git_repo_root 的整轮往返，只刷新 status。
  // repoRoots + statuses now live in the module-scope `scmCacheStore`
  // (round 42 — ε). When SourceControl unmounts (user switches off the
  // git tab) the cached snapshot survives; remounting reads it instantly
  // instead of waiting for a re-discover round-trip. The component-level
  // $derived wrappers keep template ergonomics identical.
  let repoRoots = $derived($scmCacheStore.repoRoots);
  let statuses = $derived($scmCacheStore.statuses);
  let discoveryLoading = $state(false);
  // lastCwdSignature / lastRepoSignature are likewise read from / written
  // to the cache so multiple discover passes during the same session can
  // skip redundant work even if SourceControl was unmounted in between.
  let debounceHandle: ReturnType<typeof setTimeout> | undefined;
  let inFlight: Promise<void> | null = null;
  let unlistenRepoChanged: (() => void) | undefined;
  /** Per-repo debounce for the filesystem-watcher listener (ε阶段二). A
   *  single `git commit` writes HEAD + index + refs in quick succession;
   *  coalescing 250ms ensures one refresh per user operation, not 3–5. */
  const watcherDebounce = new Map<string, ReturnType<typeof setTimeout>>();

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
    const cache = getScmCache();
    if (!force && sig === cache.lastCwdSignature && cache.repoRoots.length > 0) return;

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
      const rootsChanged = nextSig !== cache.lastRepoSignature;
      // Always update cache (signature timestamps the discovery + drops
      // stale statuses for removed repos).
      setScmRepoRoots(nextRoots, sig, nextSig);

      // Register discovered roots with the backend filesystem watcher so
      // external git changes (pull, commit from terminal, CI) trigger automatic
      // SCM refreshes without requiring user interaction.
      if (nextRoots.length > 0 && isTauri()) {
        void invoke('start_watching_repos', { roots: nextRoots }).catch(() => {});
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
      setScmRepoStatus(root, s);
      // Cascade to the pane git pill cache: stage/commit/sync writes should
      // be reflected on the pane title bar without waiting for a cwd change
      // tick. invalidate is idempotent and best-effort, so safe to fire on
      // every status read (including initial discovery).
      void invalidatePaneGitStatusForRepo(root);
    } catch (e) {
      console.error('get_scm_status failed', root, e);
    }
  }

  // ─── Repo collapse state (changes panel) ──────────────────────────────────
  let collapsedRepos = $state(new Set<string>());

  function toggleRepoCollapse(root: string): void {
    const next = new Set(collapsedRepos);
    if (next.has(root)) {
      next.delete(root);
    } else {
      next.add(root);
      // Close branch picker when collapsing so its dropdown doesn't float over empty content.
      if (branchPickerOpen === root) branchPickerOpen = '';
    }
    collapsedRepos = next;
  }

  // ─── Selected repo for GitGraph section ────────────────────────────────────
  let selectedRepo = $state('');
  // graphInfo is derived from the module-scope cache (round χ) so it survives
  // tab unmount/remount without re-fetching.
  let graphInfo = $derived<GitRepoInfo | null>($scmCacheStore.graphInfos[selectedRepo] ?? null);
  let graphLoading = $state(false);
  let graphError: string | null = $state(null);
  /**
   * Currently selected commit hash in the graph view. Derived from the
   * module-scope cache so the selection also survives tab remounts (round χ).
   * Writes go through `setScmSelectedCommit`.
   */
  let selectedCommitHash = $derived(selectedRepo ? getScmSelectedCommit(selectedRepo) : '');
  function selectCommit(hash: string): void {
    if (!selectedRepo) return;
    setScmSelectedCommit(selectedRepo, selectedCommitHash === hash ? '' : hash);
  }

  /** Clipboard write with explicit failure surfacing — Tauri webview
   *  grants clipboard-write by default, but a rejected promise without a
   *  catch was previously silent (round-32 review MEDIUM). */
  async function copyToClipboard(text: string, label: string): Promise<void> {
    try {
      if (!navigator.clipboard?.writeText) {
        throw new Error('clipboard API unavailable');
      }
      await navigator.clipboard.writeText(text);
    } catch (e) {
      await alertDialog({ title: '复制失败', message: `复制${label}失败：${e}`, danger: true });
    }
  }

  interface GitOpInProgress {
    cherry_pick: boolean;
    revert: boolean;
    merge: boolean;
    rebase: boolean;
  }

  /**
   * Wrap a backend git command + status refresh + error toast. On
   * failure, ask git whether the repo is now mid-operation
   * (cherry-pick / revert / merge / rebase paused on conflict) — if so,
   * surface a follow-up confirm offering the matching abort command.
   * Without this, a conflict left the user staring at a stderr alert
   * with no recovery path other than dropping to the terminal.
   */
  async function runCommitOp(label: string, op: () => Promise<void>): Promise<void> {
    if (!selectedRepo) return;
    try {
      await op();
    } catch (e) {
      let abortCmd: string | null = null;
      let abortMsg = '';
      try {
        const inProgress = await invoke<GitOpInProgress>('git_op_in_progress', {
          repoRoot: selectedRepo,
        });
        if (inProgress.cherry_pick) {
          abortCmd = 'git_cherry_pick_abort';
          abortMsg = '\n\n仓库目前处于 cherry-pick 暂停状态。要 abort 并恢复工作树吗？';
        } else if (inProgress.revert) {
          abortCmd = 'git_revert_abort';
          abortMsg = '\n\n仓库目前处于 revert 暂停状态。要 abort 并恢复工作树吗？';
        }
      } catch {
        /* op-status probe failed — fall through to plain error */
      }
      if (abortCmd) {
        const ok = await confirmDialog({
          title: `${label} 失败`,
          message: `${e}${abortMsg}`,
          okLabel: 'Abort',
          danger: true,
        });
        if (ok) {
          try {
            await invoke(abortCmd, { repoRoot: selectedRepo });
          } catch (abortErr) {
            await alertDialog({ title: 'Abort 失败', message: String(abortErr), danger: true });
          }
        }
      } else {
        await alertDialog({ title: `${label} 失败`, message: String(e), danger: true });
      }
    } finally {
      // Always refresh — even on partial failure the user wants to see
      // the new state (e.g. half-applied changes, unmerged files).
      await loadGraph(selectedRepo);
      await refreshStatus(selectedRepo);
      void invalidatePaneGitStatusForRepo(selectedRepo);
    }
  }

  function onCommitContextMenu(e: MouseEvent, c: CommitNode): void {
    e.preventDefault();
    e.stopPropagation();
    if (!selectedRepo) return;
    // Select the row so the user can see what they're acting on.
    setScmSelectedCommit(selectedRepo, c.hash);
    const shortHash = c.hash.slice(0, 7);
    const items: ContextMenuItem[] = [
      {
        id: 'copy-short',
        label: `复制短 hash (${shortHash})`,
        icon: Copy,
        action: () => copyToClipboard(shortHash, '短 hash'),
      },
      {
        id: 'copy-full',
        label: '复制完整 hash',
        icon: Copy,
        action: () => copyToClipboard(c.hash, '完整 hash'),
      },
      { id: 'd1', divider: true },
      {
        id: 'create-branch',
        label: '从此 commit 创建分支…',
        icon: GitBranch,
        action: () => {
          void (async () => {
            const name = await promptDialog({
              title: '创建分支',
              message: `从 ${shortHash} 创建新分支并切过去：`,
              placeholder: 'feature/my-branch',
            });
            if (!name?.trim()) return;
            await runCommitOp('创建分支', async () => {
              await invoke('git_checkout', {
                repoRoot: selectedRepo,
                branch: name.trim(),
                create: true,
                base: c.hash,
              });
            });
          })();
        },
      },
      {
        id: 'checkout-detached',
        label: 'Checkout (detached HEAD)',
        icon: GitCommit,
        action: () => {
          void (async () => {
            const ok = await confirmDialog({
              title: 'Checkout to commit',
              message: `Checkout 到 ${shortHash}？这会进入 detached HEAD 状态——你现在不会在任何分支上。`,
              okLabel: 'Checkout',
              danger: true,
            });
            if (!ok) return;
            await runCommitOp('Checkout', async () => {
              await invoke('git_checkout', {
                repoRoot: selectedRepo,
                branch: c.hash,
                create: false,
              });
            });
          })();
        },
      },
      { id: 'd2', divider: true },
      {
        id: 'cherry-pick',
        label: 'Cherry-pick',
        icon: Scissors,
        action: () => {
          void runCommitOp('Cherry-pick', async () => {
            await invoke('git_cherry_pick', {
              repoRoot: selectedRepo,
              hash: c.hash,
            });
          });
        },
      },
      {
        id: 'revert',
        label: 'Revert',
        icon: Undo2,
        action: () => {
          void (async () => {
            const ok = await confirmDialog({
              title: 'Revert commit',
              message: `Revert ${shortHash}？将创建一个反向 commit 撤销其改动。`,
              okLabel: 'Revert',
              danger: true,
            });
            if (!ok) return;
            await runCommitOp('Revert', async () => {
              await invoke('git_revert', {
                repoRoot: selectedRepo,
                hash: c.hash,
              });
            });
          })();
        },
      },
    ];
    showContextMenu(e.clientX, e.clientY, items, 'git-graph');
  }
  $effect(() => {
    if (selectedRepo) {
      setScmSelectedCommit(selectedRepo, '');
      setScmSelectedRepo(selectedRepo);
    }
  });

  async function loadGraph(root: string, { resetSelection = true } = {}): Promise<void> {
    if (!isTauri() || !root) return;
    graphLoading = true;
    graphError = null;
    // Clear selection when explicitly requested (user-triggered refresh /
    // post-commit reload). Silent background refreshes leave the selection
    // intact so the user doesn't lose their place.
    if (resetSelection) setScmSelectedCommit(root, '');
    try {
      const info = await invoke<GitRepoInfo>('get_git_info_with_cwd', { cwd: root });
      setScmGraphInfo(root, info);
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
      await alertDialog({ title: '暂存失败', message: String(e), danger: true });
    }
  }
  async function unstage(root: string, paths: string[]): Promise<void> {
    try {
      await invoke('git_unstage', { repoRoot: root, paths });
      await refreshStatus(root);
    } catch (e) {
      await alertDialog({ title: '撤销暂存失败', message: String(e), danger: true });
    }
  }
  async function discard(root: string, paths: string[]): Promise<void> {
    if (paths.length === 0) return;
    const ok = await confirmDialog({
      title: '确认丢弃',
      message: `丢弃 ${paths.length} 个文件的更改？此操作不可撤销。`,
      okLabel: '丢弃',
      danger: true,
    });
    if (!ok) return;
    try {
      await invoke('git_discard', { repoRoot: root, paths });
      await refreshStatus(root);
    } catch (e) {
      await alertDialog({ title: '丢弃失败', message: String(e), danger: true });
    }
  }
  async function commit(root: string, amend = false): Promise<void> {
    const msg = (commitMessage[root] ?? '').trim();
    if (!msg) {
      await alertDialog({ title: '请输入提交信息', message: '提交信息不能为空' });

      return;
    }
    committing = true;
    try {
      await invoke('git_commit', { repoRoot: root, message: msg, amend });
      commitMessage = { ...commitMessage, [root]: '' };
      await refreshStatus(root);
      if (root === selectedRepo) await loadGraph(root);
    } catch (e) {
      await alertDialog({ title: '提交失败', message: String(e), danger: true });
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
      await alertDialog({ title: '切换分支失败', message: String(e), danger: true });
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
      await alertDialog({ title: '创建分支失败', message: String(e), danger: true });
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
      await alertDialog({ title: '操作失败', message: `${op} 失败: ${e}`, danger: true });
    } finally {
      syncing = '';
    }
  }
  function hasUpstream(root: string, branchName: string): boolean {
    const list = branchLists[root] ?? [];
    const b = list.find((x) => x.name === branchName);
    return !!b?.upstream;
  }

  // ─── 差异预览：委托给 Monaco DiffEditorModal（VSCode 风格 side-by-side）
  // 旧的 `<pre>` 渲染路径在第 26 轮被替换；保留一个薄壳 `showDiff` 让点击
  // 处理代码无需变更——单一调用点改名为副作用更清晰，便于后续埋点。
  function showDiff(root: string, path: string, cached: boolean): void {
    openDiffEditor({ repoRoot: root, path, cached });
  }

  // 每次扫描完成后，同步加载各仓库的分支信息（供 header 显示 upstream 状态）。
  $effect(() => {
    for (const root of repoRoots) {
      if (!branchLists[root]) void loadBranches(root);
    }
  });

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
  // 分支 picker 关闭策略（VSCode 风格）：
  //   - Escape 键关闭（任何焦点位置都响应）
  //   - 鼠标按下若落在非 picker 元素上则关闭；判定方式是往上找
  //     `data-wf-branch-picker="<root>"`，相同 root 保留，其它一律关闭。
  //   在 mousedown 阶段判断而非 click —— 避免用户在外部按下、拖到 picker 内再松手
  //   被误判；也与 VSCode 的 command palette / Quick Input 的关闭时机一致。
  function onGlobalMousedown(e: MouseEvent): void {
    if (!branchPickerOpen) return;
    const t = e.target as HTMLElement | null;
    const inside = t?.closest<HTMLElement>(
      `[data-wf-branch-picker="${branchPickerOpen}"]`
    );
    if (!inside) branchPickerOpen = '';
  }
  function onGlobalKeydown(e: KeyboardEvent): void {
    if (!branchPickerOpen) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      branchPickerOpen = '';
    }
  }

  // ─── External focus-repo event ─────────────────────────────────────────
  // The pane diff pill (PaneDiffPill) dispatches `wind:scm-focus-repo` with
  // the repoRoot it wants to inspect. We:
  //   1. Make sure all the repo's groups are expanded (un-collapse them in
  //      `collapsedGroup`) so the user lands on actual file rows, not
  //      collapsed headers.
  //   2. scrollIntoView the `[data-wf-scm-repo="<root>"]` block.
  //   3. Add a transient `wf-scm-flash` class for ~1.5s as visual confirm.
  //
  // Repos may not be in the rendered list yet (race: SCM tab just opened,
  // discovery still pending). We retry with a short backoff up to 2s before
  // giving up silently.
  let flashRepo = $state<string>('');
  function focusRepo(root: string, attempt = 0): void {
    const el = document.querySelector<HTMLElement>(
      `[data-wf-scm-repo="${CSS.escape(root)}"]`
    );
    if (!el) {
      if (attempt < 8) setTimeout(() => focusRepo(root, attempt + 1), 250);
      return;
    }
    // Expand any collapsed groups so the file list is visible immediately.
    const next = new Set(collapsedGroup);
    next.delete(`${root}:staged`);
    next.delete(`${root}:changes`);
    next.delete(`${root}:untracked`);
    collapsedGroup = next;
    el.scrollIntoView({ behavior: 'smooth', block: 'start' });
    flashRepo = root;
    setTimeout(() => {
      if (flashRepo === root) flashRepo = '';
    }, 1500);
  }

  function onScmFocusRepo(e: Event): void {
    const detail = (e as CustomEvent<string>).detail;
    if (typeof detail !== 'string' || !detail) return;
    focusRepo(detail);
  }

  onMount(() => {
    // ε / round 42 — defer the initial discover when the persistent
    // cache is fresh. Tab toggles between sidebar tabs are now instant
    // because the snapshot survives unmount; only when the cache is
    // empty or stale (>30s) do we scrub-and-refresh in background.
    // round 64: removed the fresh-cache 1s background refresh to avoid
    // unsolicited proactive polling. The filesystem watcher + cwd change
    // subscriber are the two active refresh paths; mount only runs
    // discover when the cache is genuinely stale.
    if (shouldRefreshOnMount()) {
      schedule(() => discoverRepos(), 0);
    }
    // Seed the selected-repo dropdown from cache when remounting.
    if (!selectedRepo && getScmCache().repoRoots.length > 0) {
      selectedRepo = getScmCache().repoRoots[0];
      if (shouldRefreshGraphOnMount(selectedRepo)) {
        void loadGraph(selectedRepo, { resetSelection: true });
      }
    }
    // Trigger rediscovery when any pane's cwd changes (e.g. `cd`, new terminal).
    // Debounced 280ms so rapid OSC-7 bursts from multiple pane starts collapse.
    const unsub1 = paneCwdStore.subscribe(() => schedule(() => discoverRepos()));
    // Note: removed the activeWorkspaceId subscriber that previously caused an
    // immediate (0ms) forced discover on every workspace switch. Workspace
    // changes are already captured by the cwd subscriber once panes emit OSC-7,
    // and the scmCacheStore survives unmount so repeated mounts stay fast.
    document.addEventListener('mousedown', onGlobalMousedown, true);
    document.addEventListener('keydown', onGlobalKeydown);
    window.addEventListener('wind:scm-focus-repo', onScmFocusRepo as EventListener);

    // Subscribe to backend filesystem-watcher events so external git changes
    // (e.g. `git pull` from terminal, CI sync) auto-refresh the SCM panel.
    if (isTauri()) {
      void listen<string>('scm-repo-changed', (e) => {
        const changedRoot = e.payload;
        // Debounce per repo: a single `git commit` fires HEAD + index + refs
        // events in quick succession; coalesce into one refresh.
        const existing = watcherDebounce.get(changedRoot);
        if (existing !== undefined) clearTimeout(existing);
        watcherDebounce.set(changedRoot, setTimeout(async () => {
          watcherDebounce.delete(changedRoot);
          await refreshStatus(changedRoot);
          if (changedRoot === selectedRepo) {
            await loadGraph(changedRoot, { resetSelection: false });
          }
        }, 250));
      }).then((unlisten) => {
        unlistenRepoChanged = unlisten;
      });
    }

    return () => {
      unsub1();
      document.removeEventListener('mousedown', onGlobalMousedown, true);
      document.removeEventListener('keydown', onGlobalKeydown);
      window.removeEventListener('wind:scm-focus-repo', onScmFocusRepo as EventListener);
    };
  });

  onDestroy(() => {
    if (debounceHandle !== undefined) clearTimeout(debounceHandle);
    for (const t of watcherDebounce.values()) clearTimeout(t);
    watcherDebounce.clear();
    unlistenRepoChanged?.();
  });

  async function manualRefresh(): Promise<void> {
    if (inFlight) return;
    await discoverRepos(true);
    await Promise.all(repoRoots.map((root) => refreshStatus(root)));
    if (selectedRepo) await loadGraph(selectedRepo);
  }

  // When selectedRepo changes, load graph (using cache when fresh).
  $effect(() => {
    if (!selectedRepo) return;
    if (shouldRefreshGraphOnMount(selectedRepo)) {
      void loadGraph(selectedRepo, { resetSelection: true });
    } else {
      // Cache is fresh for this repo — show it; schedule background refresh.
      setTimeout(() => {
        void loadGraph(selectedRepo, { resetSelection: false });
      }, 1000);
    }
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

        <div class="flex-1 min-h-0" use:overlayScroll>
          {#if repoRoots.length === 0}
            <div class="p-4 text-[12px] text-[var(--wf-fg-muted)] text-center">
              {discoveryLoading ? '扫描中…' : '未在任意终端的 cwd 中检测到 Git 仓库。'}
            </div>
          {:else}
            {#each repoRoots as root (root)}
              {@const s = statuses[root]}
              <div
                class="scm-repo border-b border-[var(--wf-border)]/60 last:border-b-0 relative {flashRepo === root ? 'wf-scm-flash' : ''}"
                data-wf-scm-repo={root}
              >
                <!-- Repo header（VSCode 风格）：仓库名 + 分支 picker + 同步/拉取/推送
                     `sticky top-0` 让滚动正文时仓库头始终钉在可视区顶部；
                     `z-30` 高于内部 group 头（z-20），与 Explorer 的两层
                     sticky 同样的层级思路。backdrop-blur 让重叠时仍能看见
                     下方文字的轮廓而不刺眼。 -->
                <div class="sticky top-0 z-30 px-3 py-1.5 bg-[var(--wf-surface-2)]/95 backdrop-blur-md border-b border-[var(--wf-border)]/40 flex items-center gap-1.5 select-none">
                  <!-- Collapse chevron — click to fold/unfold this repo's body -->
                  <button
                    type="button"
                    class="flex items-center justify-center h-4 w-4 shrink-0 text-[var(--wf-fg-muted)] hover:text-[var(--wf-fg)] transition-colors"
                    onclick={() => toggleRepoCollapse(root)}
                    title={collapsedRepos.has(root) ? '展开' : '折叠'}
                  >
                    <ChevronRight class="h-3 w-3 transition-transform duration-150 {collapsedRepos.has(root) ? '' : 'rotate-90'}" />
                  </button>
                  <span class="text-[11px] font-semibold truncate flex-1 min-w-0" title={root}>
                    {repoName(root)}
                  </span>

                  <!-- 分支 picker 入口。data-wf-branch-picker 让全局 mousedown
                       监听识别"点击在 picker 内部"，避免点击 trigger 后立刻被自己的
                       outside-click 判定关掉。 -->
                  <button
                    type="button"
                    class="flex items-center gap-1 h-6 px-1.5 rounded text-[10px] bg-[var(--wf-accent)]/15 text-[var(--wf-accent)] hover:bg-[var(--wf-accent)]/25 transition-colors max-w-[140px]"
                    data-wf-branch-picker={root}
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

                <!-- 分支 picker 下拉（绝对定位，覆盖头部下方）。
                     ESC / 点击外部关闭逻辑见 `onGlobalMousedown` / `onGlobalKeydown`。
                     data-wf-branch-picker 标记让全局 mousedown 判定"这是 picker 内部"。 -->
                {#if branchPickerOpen === root}
                  {@const blist = branchLists[root] ?? []}
                  <div
                    class="absolute left-3 right-3 top-[34px] z-40 bg-[var(--wf-bg)] border border-[var(--wf-border)] rounded shadow-lg max-h-[260px]"
                    data-wf-branch-picker={root}
                    use:overlayScroll
                  >
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

                {#if !collapsedRepos.has(root)}
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
                      <!-- group header sticky 在 repo header 之下，z-20<30
                           保证滚动时被 repo header 盖住而不是反过来。
                           top 值与 repo header 高度（py-1.5 = 6+12+6 ≈ 24px）
                           对齐；用 wf-scm-group-sticky 类给一个具体 var 让
                           调整时不用全局 grep。 -->
                      <div class="wf-scm-group-sticky w-full flex items-center gap-1 h-6 px-3 text-[10px] font-semibold uppercase tracking-wider text-[var(--wf-fg-muted)] bg-[var(--wf-surface-2)]/92 backdrop-blur-md hover:bg-[var(--wf-surface)]/50 transition-colors">
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
                            onkeydown={(e) => e.target === e.currentTarget && e.key === 'Enter' && showDiff(root, f.path, true)}
                          >
                            <FileText class="h-3 w-3 shrink-0 text-[var(--wf-fg-muted)]" />
                            <span class="truncate text-[var(--wf-fg)]">{basename(f.path)}</span>
                            {#if dirname(f.path)}
                              <span class="text-[10px] text-[var(--wf-fg-muted)] truncate">
                                {dirname(f.path)}
                              </span>
                            {/if}
                            <!-- Right-side cluster: +N -N visible by default,
                                 actions revealed on hover. They share a
                                 single grid cell so the row width stays
                                 stable (no jumpy re-flow on hover). -->
                            <span class="ml-auto relative shrink-0 flex items-center min-h-[20px] min-w-[40px] justify-end">
                              {#if (f.additions ?? 0) > 0 || (f.deletions ?? 0) > 0}
                                <span class="flex items-center gap-0.5 font-mono text-[9px] leading-none group-hover:opacity-0 transition-opacity">
                                  {#if (f.additions ?? 0) > 0}<span class="text-emerald-400/85">+{f.additions}</span>{/if}
                                  {#if (f.deletions ?? 0) > 0}<span class="text-rose-400/85">-{f.deletions}</span>{/if}
                                </span>
                              {/if}
                              <span class="absolute inset-0 flex items-center justify-end gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity">
                                <button
                                  type="button"
                                  class="flex h-5 w-5 items-center justify-center rounded hover:bg-[var(--wf-surface)] text-[var(--wf-fg-muted)] hover:text-[var(--wf-fg)]"
                                  title="撤销暂存"
                                  onclick={(e) => { e.stopPropagation(); void unstage(root, [f.path]); }}
                                >
                                  <Minus class="h-3 w-3" />
                                </button>
                              </span>
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
                      <div class="wf-scm-group-sticky w-full flex items-center gap-1 h-6 px-3 text-[10px] font-semibold uppercase tracking-wider text-[var(--wf-fg-muted)] bg-[var(--wf-surface-2)]/92 backdrop-blur-md hover:bg-[var(--wf-surface)]/50 transition-colors">
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
                            onkeydown={(e) => e.target === e.currentTarget && e.key === 'Enter' && showDiff(root, f.path, false)}
                          >
                            <FileText class="h-3 w-3 shrink-0 text-[var(--wf-fg-muted)]" />
                            <span class="truncate text-[var(--wf-fg)]">{basename(f.path)}</span>
                            {#if dirname(f.path)}
                              <span class="text-[10px] text-[var(--wf-fg-muted)] truncate">
                                {dirname(f.path)}
                              </span>
                            {/if}
                            <span class="ml-auto relative shrink-0 flex items-center min-h-[20px] min-w-[52px] justify-end">
                              {#if (f.additions ?? 0) > 0 || (f.deletions ?? 0) > 0}
                                <span class="flex items-center gap-0.5 font-mono text-[9px] leading-none group-hover:opacity-0 transition-opacity">
                                  {#if (f.additions ?? 0) > 0}<span class="text-emerald-400/85">+{f.additions}</span>{/if}
                                  {#if (f.deletions ?? 0) > 0}<span class="text-rose-400/85">-{f.deletions}</span>{/if}
                                </span>
                              {/if}
                              <span class="absolute inset-0 flex items-center justify-end gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity">
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
                    <div class="group/grp scm-group">
                      <!-- Header restructured to match staged/changes:
                           toggle (chevron + label) is its own inner button,
                           "暂存全部" hover-only batch action sits next to
                           the count. Without this, untracked files had
                           to be staged one-by-one — friction at exactly
                           the moment users want "yes, take everything". -->
                      <div class="wf-scm-group-sticky w-full flex items-center gap-1 h-6 px-3 text-[10px] font-semibold uppercase tracking-wider text-[var(--wf-fg-muted)] bg-[var(--wf-surface-2)]/92 backdrop-blur-md hover:bg-[var(--wf-surface)]/50 transition-colors">
                        <button type="button" class="flex items-center gap-1 flex-1 text-left" onclick={() => toggleGroup(root, 'untracked')}>
                          {#if isCollapsed(root, 'untracked')}
                            <ChevronRight class="h-3 w-3" />
                          {:else}
                            <ChevronDown class="h-3 w-3" />
                          {/if}
                          <span class="flex-1">未跟踪</span>
                        </button>
                        <button
                          type="button"
                          class="flex h-5 w-5 items-center justify-center rounded opacity-0 group-hover/grp:opacity-100 hover:bg-[var(--wf-surface)] hover:text-[var(--wf-fg)] transition-all"
                          title="暂存全部未跟踪文件"
                          onclick={() => stage(root, s.untracked.map((f) => f.path))}
                        >
                          <Plus class="h-3 w-3" />
                        </button>
                        <span class="text-[var(--wf-fg)]">{s.untracked.length}</span>
                      </div>
                      {#if !isCollapsed(root, 'untracked')}
                        {#each s.untracked as f (f.path)}
                          <!-- Untracked rows are now click-to-diff like staged
                               and changes — `git_get_file_versions` with
                               cached=false treats a missing index blob as
                               empty original, rendering the entire file as
                               additions (matches VS Code's "U" file diff). -->
                          <div
                            class="group flex items-center gap-1.5 h-6 pl-6 pr-3 text-[11px] hover:bg-[var(--wf-surface)]/50 transition-colors cursor-pointer"
                            title="{f.path}（点击查看新文件 diff）"
                            role="button"
                            tabindex="0"
                            onclick={() => showDiff(root, f.path, false)}
                            onkeydown={(e) => e.target === e.currentTarget && e.key === 'Enter' && showDiff(root, f.path, false)}
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

                  {#if totalChanges === 0}
                    <div class="px-3 py-2 text-[11px] text-[var(--wf-fg-muted)]">
                      工作区干净
                    </div>
                  {/if}
                {:else}
                  <div class="px-3 py-2 text-[11px] text-[var(--wf-fg-muted)]">加载中…</div>
                {/if}
                {/if}<!-- /collapsedRepos -->
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
          <span class="text-[11px] font-semibold uppercase tracking-wider text-[var(--wf-fg-muted)] shrink-0" title="带分支线 + merge 曲线的提交图谱">
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

        <div class="flex-1 min-h-0" use:overlayScroll>
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
            <!-- Graph + rows in one flex container so the SVG aligns
                 strictly to the per-commit row baseline. Row height
                 derives from GitGraph's exported DEFAULT_DY constant —
                 single source of truth so the dots can never desync
                 from their text rows when one side is later tweaked. -->
            <div class="flex items-start min-w-max">
              <GitGraph commits={graphInfo.commits} />
              <div class="flex-1 min-w-0">
                {#each graphInfo.commits as c (c.hash)}
                  <div
                    class="flex items-center gap-1.5 pr-3 cursor-pointer transition-colors {selectedCommitHash === c.hash
                      ? 'bg-[var(--wf-accent)]/15'
                      : 'hover:bg-[var(--wf-surface)]/40'}"
                    style="height: {GRAPH_ROW_HEIGHT}px"
                    title={`${c.hash}\n${c.author} · ${formatDate(c.date)}\n右键查看操作`}
                    role="button"
                    tabindex="0"
                    onclick={() => selectCommit(c.hash)}
                    onkeydown={(e) => {
                      if (e.target !== e.currentTarget) return;
                      if (e.key === 'Enter') { selectCommit(c.hash); return; }
                      if (e.key === 'ContextMenu' || (e.key === 'F10' && e.shiftKey)) {
                        e.preventDefault();
                        const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
                        onCommitContextMenu(
                          new MouseEvent('contextmenu', { clientX: rect.left + 8, clientY: rect.bottom }),
                          c
                        );
                      }
                    }}
                    oncontextmenu={(e) => onCommitContextMenu(e, c)}
                  >
                    <!-- Ref decorations from git's `%D`: HEAD pointer +
                         local/remote branches + tags. Rendered before the
                         subject so they read like VS Code Git Graph's
                         left-aligned label cluster. Each shape gets its own
                         color treatment so HEAD vs branch vs tag is
                         distinguishable at a glance.
                         splitRefs() limits pills to MAX_VISIBLE_REFS (+ 1 when
                         HEAD is first) and returns the overflow as `hidden`. -->
                    {#each [splitRefs(c.refs)] as { visible: visibleRefs, hidden: hiddenRefs }}
                    {#each visibleRefs as ref (ref)}
                      {#if ref === 'head:'}
                        <span class="text-[9px] px-1 py-0.5 rounded bg-amber-500/20 text-amber-300 shrink-0 font-mono uppercase tracking-wider">
                          HEAD
                        </span>
                      {:else if ref.startsWith('branch:')}
                        {@const name = ref.slice(7)}
                        {@const isRemote = name.includes('/')}
                        <span class="text-[10px] px-1 py-0.5 rounded shrink-0 font-mono {isRemote
                          ? 'bg-blue-500/15 text-blue-300'
                          : 'bg-emerald-500/15 text-emerald-300'}">
                          {name}
                        </span>
                      {:else if ref.startsWith('tag:')}
                        <span class="text-[10px] px-1 py-0.5 rounded bg-violet-500/15 text-violet-300 shrink-0 font-mono">
                          ⛳ {ref.slice(4)}
                        </span>
                      {:else}
                        <!-- Future-proof fallback for ref shapes the
                             backend doesn't yet bucket (round-31 review LOW
                             — backend's parse_decorations preserves raw
                             strings, the UI now actually renders them). -->
                        <span class="text-[10px] px-1 py-0.5 rounded bg-[var(--wf-surface)] text-[var(--wf-fg-muted)] shrink-0 font-mono" title={ref}>
                          {ref}
                        </span>
                      {/if}
                    {/each}
                    {#if hiddenRefs.length > 0}
                      <span
                        class="bg-[var(--wf-surface)] text-[var(--wf-fg-muted)] text-[10px] px-1 py-0.5 rounded font-mono shrink-0"
                        title={hiddenRefs.join('\n')}
                      >+{hiddenRefs.length}</span>
                    {/if}
                    {/each}
                    <!-- Commit message: long subject lines used to truncate
                         silently. Now shift+wheel pans horizontally so the
                         user can read the tail without leaving the row.
                         `whitespace-nowrap + overflow-x-auto` keeps the
                         single-line layout; the inline-scroll listener
                         below converts vertical wheel deltas into
                         horizontal scroll only when Shift is held —
                         otherwise wheel falls through to the parent
                         vertical scroller as usual. -->
                    <span
                      class="text-[12px] text-[var(--wf-fg)] flex-1 min-w-0 text-ellipsis w-0 whitespace-nowrap overflow-x-auto wf-msg-scroll"
                      onwheel={(e) => {
                        if (!e.shiftKey) return;
                        const t = e.currentTarget as HTMLElement;
                        // Shift+wheel: convert deltaY (or deltaX if user
                        // already has a horizontal-capable wheel) into
                        // scrollLeft. preventDefault stops the page from
                        // scrolling vertically too.
                        const dx = e.deltaX !== 0 ? e.deltaX : e.deltaY;
                        t.scrollLeft += dx;
                        e.preventDefault();
                      }}
                    >
                      {c.subject}
                    </span>
                    <span class="text-[10px] font-mono text-[var(--wf-accent)]/80 shrink-0">
                      {c.hash.slice(0, 7)}
                    </span>
                    <span class="text-[10px] text-[var(--wf-fg-muted)] shrink-0 truncate max-w-[80px]">
                      {c.author}
                    </span>
                  </div>
                {/each}
              </div>
            </div>
          {/if}
        </div>
      </div>
    </SPane>
  </Splitpanes>
</div>

<!-- Diff is now handled by Monaco-backed DiffEditorModal (mounted globally
     in +page.svelte). SourceControl just calls openDiffEditor() — no
     local modal state to manage. Z-index slot 9998 unchanged. -->

<style>
  .scm-root :global(.splitpanes__splitter) {
    min-height: 1px;
    height: 1px;
    position: relative;
    transition: background-color 150ms ease;
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
    background: color-mix(in oklab, var(--wf-accent) 20%, transparent);
  }
  /* svelte-splitpanes adds `splitpanes__splitter__active` to the splitter
     while it's being dragged (see node_modules/svelte-splitpanes/dist/Pane.svelte:89).
     `:active` is included as a fallback for the brief mousedown frame before
     the library's class lands. */
  .scm-root :global(.splitpanes__splitter:active),
  .scm-root :global(.splitpanes__splitter__active) {
    background: color-mix(in oklab, var(--wf-accent) 30%, transparent);
  }
  /* PaneDiffPill 跳转过来时给目标仓库一个短暂的高亮，让用户视觉锚定。
     1.5s 内淡出，不挡住 hover 状态。 */
  .scm-root :global(.scm-repo.wf-scm-flash) {
    animation: wf-scm-flash 1.5s ease-out;
  }
  @keyframes wf-scm-flash {
    0%, 25% { background: color-mix(in oklab, var(--wf-accent) 25%, transparent); }
    100%    { background: transparent; }
  }

  /* Sticky group sub-header — pinned right under the sticky repo header
     when the user scrolls within "更改" 面板。`top` 与 repo header 高度
     对齐（py-1.5 + h-6 内容 ≈ 29px）。`position: sticky` 不能用纯
     Tailwind 因为内联类还要拼运行时 hover/transition；这里集中给一个
     class 处理位置 + 层级，模板里用 `wf-scm-group-sticky` 引用。 */
  .scm-root :global(.wf-scm-group-sticky) {
    position: sticky;
    top: 29px;
    z-index: 20;
  }
  /* Per-row commit message: hide the native horizontal scrollbar that
     `overflow-x-auto` would render — overlayscrollbars per row would be
     overkill (one instance per visible commit). The Shift+wheel handler
     is the discoverable scroll affordance. */
  .scm-root :global(.wf-msg-scroll)::-webkit-scrollbar {
    display: none;
  }
  .scm-root :global(.wf-msg-scroll) {
    scrollbar-width: none;
  }
</style>
