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
  import { portal } from '$lib/actions/portal';
  import { popupStyleFor } from '$lib/utils/anchorRect';
  import { mapLimit, GIT_FANOUT_CONCURRENCY, recommendedGitConcurrency } from '$lib/utils/pLimit';
  import { invalidatePaneGitStatusForRepo } from '$lib/stores/paneGitStatus';
  import { onFsChange, type FsChangedPayload } from '$lib/stores/fsEvents';
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
  import { fileEditorStore } from '$lib/stores/fileEditor';
  import { alertDialog, confirmDialog, promptDialog } from './RidgeDialog.svelte';
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
  let debounceHandle: ReturnType<typeof setTimeout> | undefined;
  /**
   * Controls the currently-running discovery scan. When the user `cd`s to a
   * new directory we `abort()` this so the stale scan stops launching further
   * `get_scm_status` calls instead of grinding through every subrepo of the
   * directory we already left. A fresh scan installs its own controller; the
   * abort/replace handshake lives in `runScan` + `abortActiveScan`.
   */
  let scanController: AbortController | null = null;
  let unlistenRepoChanged: (() => void) | undefined;
  let unsubCwdWatch: (() => void) | undefined;
  let unsubFsChange: (() => void) | undefined;
  /** Per-repo debounce for the filesystem-watcher listener (ε阶段二). A
   *  single `git commit` writes HEAD + index + refs in quick succession;
   *  coalescing 250ms ensures one refresh per user operation, not 3–5. */
  const watcherDebounce = new Map<string, ReturnType<typeof setTimeout>>();

  /** Cancel the in-flight discovery scan (if any) so it stops issuing git
   *  calls. Safe to call when nothing is running. */
  function abortActiveScan(): void {
    scanController?.abort();
    scanController = null;
  }

  /**
   * Start a discovery scan under a fresh AbortController, superseding any
   * scan already running. Unlike the old "drop if already running" guard,
   * a newer cwd context always wins: the previous scan is aborted, not
   * silently kept alive while its results go stale. The signal threads down
   * through `discoverRepos` → `mapLimit` so abort halts the fanout mid-flight.
   */
  function runScan(force = false): Promise<void> {
    scanController?.abort();
    const controller = new AbortController();
    scanController = controller;
    return discoverRepos(force, controller.signal).finally(() => {
      // Only clear the controller if a newer scan hasn't already replaced us.
      if (scanController === controller) scanController = null;
    });
  }

  function schedule(run: () => void, delayMs = 280): void {
    if (debounceHandle !== undefined) clearTimeout(debounceHandle);
    debounceHandle = setTimeout(() => {
      debounceHandle = undefined;
      const idle = (globalThis as unknown as { requestIdleCallback?: (cb: () => void) => number })
        .requestIdleCallback;
      if (typeof idle === 'function') idle(run);
      else run();
    }, delayMs);
  }

  /**
   * Discover git repos under the current pane cwds and refresh their status.
   *
   * `signal` makes the scan cancellable: when the user `cd`s away mid-scan,
   * `runScan` aborts the previous controller, and every checkpoint here bails
   * out *before* mutating the cache or launching more git calls. The fanouts
   * pass the signal into `mapLimit`, so an abort stops the per-repo
   * `get_scm_status` burst within one worker turn rather than draining the
   * whole backlog of an already-abandoned directory.
   */
  async function discoverRepos(force = false, signal?: AbortSignal): Promise<void> {
    if (!isTauri() || signal?.aborted) return;
    const cwds = get(paneCwdStore);
    const uniqueCwds = Array.from(new Set(Object.values(cwds).filter(Boolean))).sort();
    const sig = uniqueCwds.join('|');
    const cache = getScmCache();
    if (!force && sig === cache.lastCwdSignature && cache.repoRoots.length > 0) return;

    // Device-adaptive: high-core machines blast through the scan; 2–4 core
    // laptops keep a core free for the UI so they load progressively without
    // freezing. The backend git semaphore clamps real git.exe parallelism.
    const concurrency = recommendedGitConcurrency();

    discoveryLoading = true;
    try {
      // 按用户要求：只向下扫描当前 cwd 的子目录找 .git，不再向上找。
      // 这意味着当前 cwd 就是仓库根 / 或它的父目录集 —— 子仓库都会被发现；
      // 若用户身处 `repo/src` 这样的深子目录，则不会再把 `repo` 识别成仓库
      // （这是用户明确要求的语义：`git仓库检索不需要向上层文件夹查找，只需要向下`）。
      const found = new Map<string, number>();
      await mapLimit(
        uniqueCwds,
        concurrency,
        async (cwd) => {
          if (signal?.aborted) return;
          try {
            const roots = await invoke<string[]>('find_git_repos_below', { path: cwd, maxDepth: 4 });
            if (signal?.aborted) return;
            for (const r of roots) {
              found.set(r, (found.get(r) ?? 0) + 1);
            }
          } catch {
            /* ignore */
          }
        },
        { signal }
      );
      // Bail before touching the cache: writing repo roots for a directory the
      // user already left would flash stale repos into the sidebar.
      if (signal?.aborted) return;

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

      // Cap concurrent `get_scm_status` fanout: each call spawns ~3 git.exe
      // and Windows `CreateProcess` is slow enough that a 20-repo parallel
      // burst saturates tokio's blocking pool, freezing the Explorer sidebar
      // (which queues behind us). The signal lets a directory switch abort the
      // remaining per-repo refreshes. See `src/lib/utils/pLimit.ts`.
      await mapLimit(nextRoots, concurrency, (root) => refreshStatus(root), { signal });
      if (signal?.aborted) return;
      if (rootsChanged && selectedRepo) await loadGraph(selectedRepo);
    } finally {
      // Leave the spinner up if a newer scan superseded us — that scan owns
      // the flag now and will clear it when it finishes.
      if (!signal?.aborted) discoveryLoading = false;
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
   * Graph 刷新按钮 in-flight 标记。loadGraph 自身用 graphLoading 表示「拉本地
   * commits」阶段；这个标记额外覆盖 git pull → refreshStatus → loadBranches 三
   * 步前置流程，让 spinner 在整个复合操作里都转动。
   */
  let graphRefreshing = $state(false);
  /**
   * Currently selected commit hash in the graph view. Derived from the
   * module-scope cache so the selection also survives tab remounts (round χ).
   * Writes go through `setScmSelectedCommit`.
   */
  // Derive from the reactive store so Svelte tracks the dependency and
  // re-evaluates when selectCommit() writes to it. getScmSelectedCommit()
  // uses get() (one-shot) and would NOT re-run on store changes.
  let selectedCommitHash = $derived(selectedRepo ? ($scmCacheStore.selectedCommitHashByRepo[selectedRepo] ?? '') : '');
  function selectCommit(hash: string): void {
    if (!selectedRepo) return;
    setScmSelectedCommit(selectedRepo, selectedCommitHash === hash ? '' : hash);
  }

  // ─── Commit inline 详情面板（VS Code GitGraph 风格）───────────────────────
  // 单击 commit 后展开一行 240px 的详情区域，列出 commit 涉及的文件。
  // GitGraph 的 layoutGraph 收到 expandedHash + expandedExtra，将该行下方腾出
  // 同等高度，dot 与 commit-meta 永远 1:1 对齐。
  const COMMIT_EXPAND_PX = 240;
  interface CommitFileBag {
    loading: boolean;
    files: { path: string; status: string }[];
    error: string | null;
  }
  let commitFilesCache = $state(new Map<string, CommitFileBag>());

  async function loadCommitFiles(repoRoot: string, hash: string): Promise<void> {
    const cacheKey = `${repoRoot}::${hash}`;
    if (commitFilesCache.get(cacheKey)) return; // already loaded / loading
    const next = new Map(commitFilesCache);
    next.set(cacheKey, { loading: true, files: [], error: null });
    commitFilesCache = next;
    try {
      const files = await invoke<{ path: string; status: string }[]>(
        'git_get_commit_files',
        { repoRoot, hash }
      );
      const m = new Map(commitFilesCache);
      m.set(cacheKey, { loading: false, files, error: null });
      commitFilesCache = m;
    } catch (e) {
      const m = new Map(commitFilesCache);
      m.set(cacheKey, { loading: false, files: [], error: String(e) });
      commitFilesCache = m;
    }
  }

  $effect(() => {
    if (selectedRepo && selectedCommitHash) {
      void loadCommitFiles(selectedRepo, selectedCommitHash);
    }
  });

  function commitFilesFor(hash: string): CommitFileBag | undefined {
    if (!selectedRepo) return undefined;
    return commitFilesCache.get(`${selectedRepo}::${hash}`);
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
      { id: 'd3', divider: true },
      {
        id: 'view-diff',
        label: 'View commit diff',
        icon: FileText,
        action: () => {
          void (async () => {
            // 如果还没拉文件列表，先拉一次（与 inline 展开共享缓存）。
            if (!commitFilesFor(c.hash)) {
              await loadCommitFiles(selectedRepo!, c.hash);
            }
            const bag = commitFilesFor(c.hash);
            if (!bag || bag.files.length === 0) {
              await alertDialog({
                title: '无变动文件',
                message: bag?.error ?? `${shortHash} 不包含可显示的文件改动。`,
              });
              return;
            }
            // 一次打开多个 tab 体验差，先打开第一个；用户可在 inline 面板里逐个看。
            fileEditorStore.openDiffTab({
              repoRoot: selectedRepo!,
              path: bag.files[0].path,
              cached: false,
              commit: c.hash,
            });
            // 同步在图谱里高亮这个 commit，用户能看到详情面板里的完整文件列表。
            setScmSelectedCommit(selectedRepo!, c.hash);
          })();
        },
      },
      {
        id: 'create-tag',
        label: 'Create tag…',
        icon: Tag,
        action: () => {
          void (async () => {
            const name = await promptDialog({
              title: '创建 tag',
              message: `在 ${shortHash} 上创建标签：`,
              placeholder: 'v1.0.0',
            });
            if (!name?.trim()) return;
            const message = await promptDialog({
              title: 'Annotated tag 信息',
              message: '可选。留空则创建 lightweight tag（无 message）。',
              placeholder: 'Release v1.0.0',
            });
            await runCommitOp('创建 tag', async () => {
              await invoke('git_create_tag', {
                repoRoot: selectedRepo,
                name: name.trim(),
                hash: c.hash,
                message: message?.trim() || null,
              });
            });
          })();
        },
      },
      {
        id: 'reset',
        label: 'Reset 到此 commit',
        icon: RotateCw,
        children: [
          {
            id: 'reset-soft',
            label: 'Soft  (保留索引与工作区改动)',
            action: () => {
              void runCommitOp('Reset --soft', async () => {
                await invoke('git_reset', {
                  repoRoot: selectedRepo,
                  hash: c.hash,
                  mode: 'soft',
                });
              });
            },
          },
          {
            id: 'reset-mixed',
            label: 'Mixed (保留工作区改动，清空索引)',
            action: () => {
              void runCommitOp('Reset --mixed', async () => {
                await invoke('git_reset', {
                  repoRoot: selectedRepo,
                  hash: c.hash,
                  mode: 'mixed',
                });
              });
            },
          },
          {
            id: 'reset-hard',
            label: 'Hard  (丢弃所有未提交改动 ‼)',
            action: () => {
              void (async () => {
                const ok = await confirmDialog({
                  title: 'Reset --hard',
                  message: `Reset --hard 到 ${shortHash}？\n\n会丢弃所有未提交的改动，且无法恢复。`,
                  okLabel: 'Reset --hard',
                  danger: true,
                });
                if (!ok) return;
                await runCommitOp('Reset --hard', async () => {
                  await invoke('git_reset', {
                    repoRoot: selectedRepo,
                    hash: c.hash,
                    mode: 'hard',
                  });
                });
              })();
            },
          },
        ],
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
      // 重置"已到底"标记 —— 重新载图后允许再次往下分页。
      noMoreCommits.delete(root);
    } catch (e) {
      graphError = String(e);
    } finally {
      graphLoading = false;
    }
  }

  /**
   * 图谱刷新按钮 = 先 best-effort `git pull --ff-only`，失败也继续 reload，
   * 这样：
   *   • 有 upstream + 网络通时一键拉到最新远端 commits；
   *   • 离线 / 无 upstream / 冲突时只是错过 pull，graph 仍按本地状态刷新。
   * pull 失败不弹 alertDialog（runSync 那条路径有），仅 console.warn —— 用户
   * 期待这里是「刷新」语义，不是「我要解决冲突」语义。
   */
  async function refreshGraphWithPull(root: string): Promise<void> {
    if (!root || graphRefreshing) return;
    graphRefreshing = true;
    try {
      if (isTauri()) {
        try {
          await invoke('git_pull', { repoRoot: root });
        } catch (e) {
          console.warn('refreshGraphWithPull: git pull failed', e);
        }
      }
      await refreshStatus(root);
      await loadBranches(root);
      await loadGraph(root);
    } finally {
      graphRefreshing = false;
    }
  }

  // T10：滚动 sentinel action —— 元素进入视口时调一次回调。Observer 复用同一
  // 实例避免泄漏。回调由 SourceControl 注入 `loadMoreCommits`。
  function rgGraphSentinel(node: HTMLElement, onEnter: () => void) {
    let cb = onEnter;
    const io = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) cb();
        }
      },
      { rootMargin: '0px 0px 100px 0px', threshold: 0 }
    );
    io.observe(node);
    return {
      update(next: () => void) {
        cb = next;
      },
      destroy() {
        io.disconnect();
      },
    };
  }

  // T10：图谱无限滚动 —— sentinel 进入视口时调 get_git_commits_paginated
  // 把更早的 commits append 到现有数组。空返回 → 标记该 repo 已到底。
  let loadingMoreCommits = $state(false);
  const noMoreCommits = new Set<string>();
  async function loadMoreCommits(root: string): Promise<void> {
    if (!isTauri() || !root || loadingMoreCommits) return;
    if (noMoreCommits.has(root)) return;
    const cur = $scmCacheStore.graphInfos[root];
    if (!cur) return;
    loadingMoreCommits = true;
    try {
      const more = await invoke<typeof cur.commits>('get_git_commits_paginated', {
        repoRoot: root,
        offset: cur.commits.length,
        limit: 100,
      });
      if (more.length === 0) {
        noMoreCommits.add(root);
        return;
      }
      // append 后写回 store；spread 复制确保 reactive 触发。
      const next = { ...cur, commits: [...cur.commits, ...more] };
      setScmGraphInfo(root, next);
    } catch (e) {
      console.warn('loadMoreCommits failed', e);
    } finally {
      loadingMoreCommits = false;
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
  /**
   * 撤销一组改动。区分 tracked vs untracked：
   *  - tracked → `git checkout -- <paths>`（恢复到 HEAD/索引）
   *  - untracked → `git clean -fd -- <paths>`（**永久删除**未跟踪文件 / 空目录）
   * untracked 删除是不可逆的（没有 HEAD 版本可恢复），提示文案据此区分。
   */
  async function discard(root: string, files: ScmFile[]): Promise<void> {
    if (files.length === 0) return;
    const tracked = files.filter((f) => f.group !== 'untracked');
    const untracked = files.filter((f) => f.group === 'untracked');
    const message =
      untracked.length > 0
        ? tracked.length > 0
          ? `丢弃 ${files.length} 个文件的更改？将永久删除 ${untracked.length} 个未跟踪文件，此操作不可撤销。`
          : `永久删除 ${untracked.length} 个未跟踪文件？此操作不可撤销。`
        : `丢弃 ${files.length} 个文件的更改？此操作不可撤销。`;
    const ok = await confirmDialog({
      title: untracked.length > 0 && tracked.length === 0 ? '永久删除未跟踪文件' : '确认丢弃',
      message,
      okLabel: untracked.length > 0 && tracked.length === 0 ? '删除' : '丢弃',
      danger: true,
    });
    if (!ok) return;
    try {
      if (tracked.length > 0) {
        await invoke('git_discard', {
          repoRoot: root,
          paths: tracked.map((f) => f.path),
        });
      }
      if (untracked.length > 0) {
        await invoke('git_clean_untracked', {
          repoRoot: root,
          paths: untracked.map((f) => f.path),
        });
      }
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
  let pickerAnchor: HTMLElement | undefined = $state();
  let creatingBranchName = $state<string | null>(null); // null = not creating; '' or 'foo' = inline input visible
  let creatingBranchRoot = $state<string>('');
  let pendingCreateCommit = $state(false);
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
  async function openBranchPicker(root: string, ev: MouseEvent): Promise<void> {
    if (branchPickerOpen === root) {
      branchPickerOpen = '';
      creatingBranchName = null;
      return;
    }
    pickerAnchor = ev.currentTarget as HTMLElement;
    branchPickerOpen = root;
    creatingBranchName = null;
    creatingBranchRoot = '';
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
  function startCreateBranch(root: string): void {
    creatingBranchRoot = root;
    creatingBranchName = '';
  }
  function cancelCreateBranch(): void {
    creatingBranchName = null;
    creatingBranchRoot = '';
  }
  async function commitCreateBranch(): Promise<void> {
    if (pendingCreateCommit) return;
    const name = (creatingBranchName ?? '').trim();
    const root = creatingBranchRoot;
    if (!name || !root) {
      cancelCreateBranch();
      return;
    }
    pendingCreateCommit = true;
    try {
      branchPickerOpen = '';
      creatingBranchName = null;
      creatingBranchRoot = '';
      await invoke('git_checkout', { repoRoot: root, branch: name, create: true });
      await refreshStatus(root);
      await loadBranches(root);
      if (root === selectedRepo) await loadGraph(root);
    } catch (e) {
      await alertDialog({ title: '创建分支失败', message: String(e), danger: true });
    } finally {
      pendingCreateCommit = false;
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
  // 历史问题：N 个仓库一次性 `void loadBranches(root)` 会并发触发 N 个
  // `git branch --all` 进程，与 refreshStatus 的 ~3N 个 git 进程叠加，在
  // Windows 上 CreateProcess 是瓶颈，整批 spawn 会阻塞 tokio blocking pool
  // 并把 Explorer 的 get_file_tree 一起拖死。改用 GIT_FANOUT_CONCURRENCY
  // 控制并发，与后端 git 信号量保持同步。
  $effect(() => {
    const pending = repoRoots.filter((root) => !branchLists[root]);
    if (pending.length === 0) return;
    void mapLimit(pending, GIT_FANOUT_CONCURRENCY, (root) => loadBranches(root));
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
      default: return 'text-[var(--rg-fg-muted)]';
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
  //     `data-rg-branch-picker="<root>"`，相同 root 保留，其它一律关闭。
  //   在 mousedown 阶段判断而非 click —— 避免用户在外部按下、拖到 picker 内再松手
  //   被误判；也与 VSCode 的 command palette / Quick Input 的关闭时机一致。
  function onGlobalMousedown(e: MouseEvent): void {
    if (!branchPickerOpen) return;
    const t = e.target as HTMLElement | null;
    const inside = t?.closest<HTMLElement>(
      `[data-rg-branch-picker="${branchPickerOpen}"]`
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
  // The pane diff pill (PaneDiffPill) dispatches `ridge:scm-focus-repo` with
  // the repoRoot it wants to inspect. We:
  //   1. Make sure all the repo's groups are expanded (un-collapse them in
  //      `collapsedGroup`) so the user lands on actual file rows, not
  //      collapsed headers.
  //   2. scrollIntoView the `[data-rg-scm-repo="<root>"]` block.
  //   3. Add a transient `rg-scm-flash` class for ~1.5s as visual confirm.
  //
  // Repos may not be in the rendered list yet (race: SCM tab just opened,
  // discovery still pending). We retry with a short backoff up to 2s before
  // giving up silently.
  let flashRepo = $state<string>('');
  function focusRepo(root: string, attempt = 0): void {
    const el = document.querySelector<HTMLElement>(
      `[data-rg-scm-repo="${CSS.escape(root)}"]`
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
  const cache = getScmCache();
  if (cache.repoRoots.length === 0) {
    void runScan();
  } else {
    if (!selectedRepo) selectedRepo = cache.repoRoots[0];
    if (selectedRepo && shouldRefreshGraphOnMount(selectedRepo)) {
      void loadGraph(selectedRepo, { resetSelection: true });
    }
    // 用户隐藏 SCM tab 期间，工作区文件可能已被外部修改但 fs-changed 订阅
    // 已随组件 unmount 释放，cache 里的 status 已陈旧。切回时静默并发刷一次，
    // 不设 loading flag、不显示 spinner —— 数据替换由 setScmRepoStatus 直接生效。
    // 并发同样受 GIT_FANOUT_CONCURRENCY 约束，避免多仓库 SCM tab remount
    // 触发 N 个 get_scm_status 同时启动 ~3N 个 git.exe。
    void mapLimit(cache.repoRoots, GIT_FANOUT_CONCURRENCY, (r) => refreshStatus(r));
  }

  // 监听 paneCwdStore：cwd 集合变化（新增/移除/cd 切换）时重新发现仓库。
  // 关键修复：用户在终端 `cd` 离开一个多仓库目录后，之前那轮扫描必须立刻
  // abort —— 否则它会继续把已离开目录下几十个子仓库的 get_scm_status 跑完，
  // 既浪费 git.exe 又让 sidebar 一直转圈。这里在察觉到任何集合变化的瞬间
  // 同步 abortActiveScan()，再 debounce 一个新的 runScan（runScan 内部也会
  // 再 abort 一次，双保险）。
  let knownCwds = new Set(Object.values(get(paneCwdStore)).filter(Boolean));
  unsubCwdWatch = paneCwdStore.subscribe((cwds) => {
    const current = new Set(Object.values(cwds).filter(Boolean));
    // 集合是否变化：大小不同，或出现了未知 cwd。覆盖新增、移除、cd 切换。
    let changed = current.size !== knownCwds.size;
    if (!changed) {
      for (const cwd of current) {
        if (!knownCwds.has(cwd)) { changed = true; break; }
      }
    }
    knownCwds = current;
    if (changed) {
      // 立即取消正在跑的过期扫描，让 git.exe 与 blocking pool 尽快空出来。
      abortActiveScan();
      schedule(() => void runScan());
    }
  });

  // 订阅通用文件系统事件：用户在 Ridge 内或外部编辑/创建/删除工作区文件时，
  // 对应仓库的 status 立刻刷新（VS Code "Changes" 面板的实时表现）。
  // GitWatcher 只看 .git，看不到工作区文件变化，所以这条独立路径必须存在。
  unsubFsChange = onFsChange((payload: FsChangedPayload) => {
    if (repoRoots.length === 0) return;
    // 把"哪些路径触发了变化"映射成"哪些 repo 受影响"。`coalesced` 时退化为
    // 整个 fs-changed root，按前缀匹配所有命中的 repo（同一 cwd 下可能多 repo）。
    const probes = payload.coalesced ? [payload.root] : payload.paths;
    const hit = new Set<string>();
    for (const raw of probes) {
      const probe = raw.replace(/\\/g, '/');
      let best: string | null = null;
      for (const r of repoRoots) {
        const root = r.replace(/\\/g, '/');
        if (probe === root || probe.startsWith(root + '/')) {
          if (!best || root.length > best.length) best = r;
        }
      }
      if (best) hit.add(best);
    }
    for (const root of hit) {
      const prev = watcherDebounce.get(root);
      if (prev) clearTimeout(prev);
      watcherDebounce.set(
        root,
        setTimeout(() => {
          watcherDebounce.delete(root);
          void refreshStatus(root);
        }, 250)
      );
    }
  });

  // 订阅后端 GitWatcher emit 的 scm-repo-changed：外部 git 操作（如另一终端 commit、
  // 文件管理器改 .gitignore）也能让 SCM 面板自动刷新。watcherDebounce 在 commit 这种
  // 一次操作触发 HEAD/index/refs 多次写入时合并为一次 refresh。
  if (isTauri()) {
    void (async () => {
      try {
        unlistenRepoChanged = await listen<string>('scm-repo-changed', (evt) => {
          const root = evt.payload;
          if (!root) return;
          const prev = watcherDebounce.get(root);
          if (prev) clearTimeout(prev);
          watcherDebounce.set(
            root,
            setTimeout(() => {
              watcherDebounce.delete(root);
              void refreshStatus(root);
              // Graph NOT refreshed here — external git changes (e.g. terminal
              // commit) would cause rapid auto-reloads. Graph updates only on:
              // manual refresh button or SCM-panel git operations (commit,
              // branch switch, sync). Initial discovery handles rootsChanged.
            }, 250)
          );
        });
      } catch (e) {
        console.warn('listen scm-repo-changed failed', e);
      }
    })();
  }

  document.addEventListener('mousedown', onGlobalMousedown, true);
  document.addEventListener('keydown', onGlobalKeydown);
  window.addEventListener('ridge:scm-focus-repo', onScmFocusRepo as EventListener);

  return () => {
    document.removeEventListener('mousedown', onGlobalMousedown, true);
    document.removeEventListener('keydown', onGlobalKeydown);
    window.removeEventListener('ridge:scm-focus-repo', onScmFocusRepo as EventListener);
  };
});

  onDestroy(() => {
    if (debounceHandle !== undefined) clearTimeout(debounceHandle);
    // 组件卸载（用户切走 SCM tab）时，取消正在跑的扫描，别让它在后台继续
    // 烧 git.exe。
    abortActiveScan();
    for (const t of watcherDebounce.values()) clearTimeout(t);
    watcherDebounce.clear();
    unlistenRepoChanged?.();
    unsubCwdWatch?.();
    unsubFsChange?.();
  });

  async function manualRefresh(): Promise<void> {
    // 手动刷新走与 cwd 扫描相同的可取消通道：runScan(force=true) 内部会
    // abort 上一轮、装新 controller，并用自适应并发刷新所有仓库的 status。
    // discoverRepos(force) 已经 refresh 全部 roots，无需再重复一遍。
    await runScan(true);
    if (selectedRepo && !scanController) await loadGraph(selectedRepo);
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

<div class="scm-root flex flex-col h-full min-h-0 rg-git-graph">
  <Splitpanes horizontal={true} theme="" class="rg-split flex-1 min-h-0">
    <!-- ═══ Top: Changes section ═══ -->
    <SPane size={50} minSize={20}>
      <div class="flex flex-col h-full min-h-0">
        <div
          class="px-3 h-9 shrink-0 flex items-center justify-between border-b border-[var(--rg-border)] bg-[var(--rg-surface)]/40"
        >
          <span class="text-[11px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)]">
            更改
          </span>
          <div class="flex items-center gap-1">
            <span class="text-[10px] text-[var(--rg-fg-muted)]">{repoRoots.length} 仓库</span>
            <button
              type="button"
              class="flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)]"
              title="刷新"
              onclick={() => void manualRefresh()}
            >
              <RefreshCw class="h-3 w-3 {discoveryLoading ? 'animate-spin' : ''}" />
            </button>
          </div>
        </div>

        <div class="flex-1 min-h-0" use:overlayScroll>
          {#if repoRoots.length === 0}
            <div class="p-4 text-[12px] text-[var(--rg-fg-muted)] text-center">
              {discoveryLoading ? '扫描中…' : '未在任意终端的 cwd 中检测到 Git 仓库。'}
            </div>
          {:else}
            {#each repoRoots as root (root)}
              {@const s = statuses[root]}
              <div
                class="scm-repo border-b border-[var(--rg-border)]/60 last:border-b-0 relative {flashRepo === root ? 'rg-scm-flash' : ''}"
                data-rg-scm-repo={root}
              >
                <!-- Repo header（VSCode 风格）：仓库名 + 分支 picker + 同步/拉取/推送
                     `sticky top-0` 让滚动正文时仓库头始终钉在可视区顶部；
                     `z-30` 高于内部 group 头（z-20），与 Explorer 的两层
                     sticky 同样的层级思路。backdrop-blur 让重叠时仍能看见
                     下方文字的轮廓而不刺眼。 -->
                <div class="sticky top-0 z-30 px-3 py-1.5 bg-[var(--rg-surface-2)]/95 backdrop-blur-md border-b border-[var(--rg-border)]/40 flex items-center gap-1.5 select-none">
                  <!-- Collapse chevron — click to fold/unfold this repo's body -->
                  <button
                    type="button"
                    class="flex items-center justify-center h-4 w-4 shrink-0 text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] transition-colors"
                    onclick={() => toggleRepoCollapse(root)}
                    title={collapsedRepos.has(root) ? '展开' : '折叠'}
                  >
                    <ChevronRight class="h-3 w-3 transition-transform duration-150 {collapsedRepos.has(root) ? '' : 'rotate-90'}" />
                  </button>
                  <span class="text-[11px] font-semibold truncate flex-1 min-w-0" title={root}>
                    {repoName(root)}
                  </span>

                  <!-- 分支 picker 入口。data-rg-branch-picker 让全局 mousedown
                       监听识别"点击在 picker 内部"，避免点击 trigger 后立刻被自己的
                       outside-click 判定关掉。 -->
                  <button
                    type="button"
                    class="flex items-center gap-1 h-6 px-1.5 rounded text-[10px] bg-[var(--rg-accent)]/15 text-[var(--rg-accent)] hover:bg-[var(--rg-accent)]/25 transition-colors max-w-[140px]"
                    data-rg-branch-picker={root}
                    onclick={(ev) => void openBranchPicker(root, ev)}
                    title={s?.current_branch ? `当前分支：${s.current_branch}（点击切换）` : '切换分支'}
                  >
                    <GitBranch class="h-3 w-3 shrink-0" />
                    <span class="truncate">{s?.current_branch ?? '(detached)'}</span>
                  </button>

                  <!-- 上/下箭头显示 ahead/behind；点击触发 sync -->
                  {#if s && (s.ahead > 0 || s.behind > 0)}
                    <button
                      type="button"
                      class="flex items-center gap-0.5 h-6 px-1.5 rounded text-[10px] border border-[var(--rg-border)] text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)] transition-colors"
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
                    class="flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)]"
                    onclick={() => void runSync(root, 'fetch')}
                    disabled={syncing === root}
                    title="Fetch（git fetch --all --prune）"
                  >
                    <RotateCw class="h-3 w-3 {syncing === root ? 'animate-spin' : ''}" />
                  </button>
                  <button
                    type="button"
                    class="flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)]"
                    onclick={() => void runSync(root, 'pull')}
                    disabled={syncing === root}
                    title="Pull（git pull --ff-only）"
                  >
                    <ArrowDown class="h-3 w-3" />
                  </button>
                  <button
                    type="button"
                    class="flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)]"
                    onclick={() => void runSync(root, 'push')}
                    disabled={syncing === root}
                    title="Push（无 upstream 时自动 -u origin HEAD）"
                  >
                    <ArrowUp class="h-3 w-3" />
                  </button>
                </div>

                <!-- 分支 picker 下拉。portaled 到 body，避免被 sidebar overflow 裁剪；
                     位置由 pickerAnchor + popupStyleFor 计算（bottom-start, gap=4）。
                     ESC / 点击外部关闭逻辑见 `onGlobalMousedown` / `onGlobalKeydown`。
                     data-rg-branch-picker 标记让全局 mousedown 判定"这是 picker 内部"。 -->
                {#if branchPickerOpen === root && pickerAnchor}
                  {@const blist = branchLists[root] ?? []}
                  <div
                    class="z-[9990] bg-[var(--rg-bg)] border border-[var(--rg-border)] rounded shadow-lg max-h-[260px]"
                    style={popupStyleFor(pickerAnchor, 'bottom-start') + ';width:240px'}
                    data-rg-branch-picker={root}
                    data-rg-portal-id={`branch-picker:${root}`}
                    use:portal={{ id: `branch-picker:${root}` }}
                    use:overlayScroll
                  >
                    {#if creatingBranchName !== null && creatingBranchRoot === root}
                      <div
                        class="flex items-center gap-1.5 px-3 h-7 text-[11px] text-[var(--rg-accent)] border-b border-[var(--rg-border)]/60"
                        data-rg-branch-picker={root}
                      >
                        <Plus class="h-3 w-3 shrink-0" />
                        <!-- svelte-ignore a11y_autofocus -->
                        <input
                          type="text"
                          autofocus
                          class="flex-1 bg-transparent border-0 outline-none text-[var(--rg-fg)] placeholder-[var(--rg-fg-muted)]/60"
                          placeholder="新分支名称"
                          bind:value={creatingBranchName}
                          onkeydown={(ev) => {
                            if (ev.key === 'Enter') {
                              ev.preventDefault();
                              void commitCreateBranch();
                            } else if (ev.key === 'Escape') {
                              ev.preventDefault();
                              cancelCreateBranch();
                            }
                          }}
                          onblur={() => {
                            if (!pendingCreateCommit) cancelCreateBranch();
                          }}
                        />
                      </div>
                    {:else}
                      <button
                        type="button"
                        class="w-full flex items-center gap-1.5 px-3 h-7 text-[11px] text-[var(--rg-accent)] hover:bg-[var(--rg-surface)] border-b border-[var(--rg-border)]/60 transition-colors"
                        data-rg-branch-picker={root}
                        onclick={() => startCreateBranch(root)}
                      >
                        <Plus class="h-3 w-3" /> 创建新分支…
                      </button>
                    {/if}
                    {#each blist as b (b.name)}
                      <button
                        type="button"
                        class="group w-full flex items-center gap-1.5 px-3 h-7 text-[11px] text-[var(--rg-fg)] hover:bg-[var(--rg-surface)] transition-colors"
                        data-rg-branch-picker={root}
                        onclick={() => void switchBranch(root, b.name)}
                      >
                        {#if b.is_current}
                          <Check class="h-3 w-3 text-[var(--rg-accent)]" />
                        {:else}
                          <span class="w-3"></span>
                        {/if}
                        <GitBranch class="h-3 w-3 shrink-0 {b.is_remote ? 'text-blue-400/70' : 'text-[var(--rg-fg-muted)]'}" />
                        <span class="truncate flex-1 text-left">{b.name}</span>
                        {#if b.upstream}
                          <span class="text-[9px] text-[var(--rg-fg-muted)]/70 truncate">→ {b.upstream}</span>
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
                    <div class="px-3 py-2 flex flex-col gap-1.5 border-b border-[var(--rg-border)]/40">
                      <input
                        type="text"
                        class="w-full text-[12px] px-2 py-1 rounded bg-[var(--rg-bg)] border border-[var(--rg-border)] text-[var(--rg-fg)] focus:outline-none focus:border-[var(--rg-accent)]/60"
                        placeholder="消息（仅提交已暂存的更改）"
                        bind:value={commitMessage[root]}
                      />
                      <div class="flex items-center gap-1.5">
                        <button
                          type="button"
                          class="flex-1 flex items-center justify-center gap-1 px-2 py-1 rounded text-[11px] bg-[var(--rg-accent)]/15 text-[var(--rg-accent)] border border-[var(--rg-accent)]/30 hover:bg-[var(--rg-accent)]/25 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
                          onclick={() => commit(root, false)}
                          disabled={committing || s.staged.length === 0}
                          title={s.staged.length === 0 ? '请先暂存文件' : '提交已暂存的更改'}
                        >
                          <GitCommit class="h-3 w-3" /> 提交 {s.staged.length}
                        </button>
                        <button
                          type="button"
                          class="px-2 py-1 rounded text-[10px] border border-[var(--rg-border)] text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)] disabled:opacity-40"
                          onclick={() => commit(root, true)}
                          disabled={committing || s.staged.length === 0}
                          title="修改最近一次提交（git commit --amend）"
                        >
                          Amend
                        </button>
                        {#if s.changes.length + s.untracked.length > 0}
                          <button
                            type="button"
                            class="px-2 py-1 rounded text-[11px] border border-[var(--rg-border)] text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)]"
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
                           对齐；用 rg-scm-group-sticky 类给一个具体 var 让
                           调整时不用全局 grep。 -->
                      <div class="rg-scm-group-sticky w-full flex items-center gap-1 h-6 px-3 text-[10px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)] bg-[var(--rg-surface-2)]/92 backdrop-blur-md hover:bg-[var(--rg-surface)]/50 transition-colors">
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
                          class="flex h-5 w-5 items-center justify-center rounded opacity-0 group-hover/grp:opacity-100 hover:bg-[var(--rg-surface)] hover:text-[var(--rg-fg)] transition-all"
                          title="撤销暂存全部"
                          onclick={() => unstage(root, s.staged.map((f) => f.path))}
                        >
                          <Minus class="h-3 w-3" />
                        </button>
                        <span class="text-[var(--rg-fg)]">{s.staged.length}</span>
                      </div>
                      {#if !isCollapsed(root, 'staged')}
                        {#each s.staged as f (f.path)}
                          <div
                            class="group flex items-center gap-1.5 h-6 pl-6 pr-3 text-[11px] hover:bg-[var(--rg-surface)]/50 transition-colors cursor-pointer"
                            title="{f.path}（点击查看差异）"
                            role="button"
                            tabindex="0"
                            onclick={() => void showDiff(root, f.path, true)}
                            onkeydown={(e) => e.target === e.currentTarget && e.key === 'Enter' && showDiff(root, f.path, true)}
                          >
                            <FileText class="h-3 w-3 shrink-0 text-[var(--rg-fg-muted)]" />
                            <span class="truncate text-[var(--rg-fg)]">{basename(f.path)}</span>
                            {#if dirname(f.path)}
                              <span class="text-[10px] text-[var(--rg-fg-muted)] truncate">
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
                                  class="flex h-5 w-5 items-center justify-center rounded hover:bg-[var(--rg-surface)] text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]"
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
                      <div class="rg-scm-group-sticky w-full flex items-center gap-1 h-6 px-3 text-[10px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)] bg-[var(--rg-surface-2)]/92 backdrop-blur-md hover:bg-[var(--rg-surface)]/50 transition-colors">
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
                          class="flex h-5 w-5 items-center justify-center rounded opacity-0 group-hover/grp:opacity-100 hover:bg-[var(--rg-surface)] hover:text-red-400 transition-all"
                          title="丢弃全部未暂存更改"
                          onclick={() => discard(root, s.changes)}
                        >
                          <Undo2 class="h-3 w-3" />
                        </button>
                        <button
                          type="button"
                          class="flex h-5 w-5 items-center justify-center rounded opacity-0 group-hover/grp:opacity-100 hover:bg-[var(--rg-surface)] hover:text-[var(--rg-fg)] transition-all"
                          title="暂存全部"
                          onclick={() => stage(root, s.changes.map((f) => f.path))}
                        >
                          <Plus class="h-3 w-3" />
                        </button>
                        <span class="text-[var(--rg-fg)]">{s.changes.length}</span>
                      </div>
                      {#if !isCollapsed(root, 'changes')}
                        {#each s.changes as f (f.path)}
                          <div
                            class="group flex items-center gap-1.5 h-6 pl-6 pr-3 text-[11px] hover:bg-[var(--rg-surface)]/50 transition-colors cursor-pointer"
                            title="{f.path}（点击查看差异）"
                            role="button"
                            tabindex="0"
                            onclick={() => void showDiff(root, f.path, false)}
                            onkeydown={(e) => e.target === e.currentTarget && e.key === 'Enter' && showDiff(root, f.path, false)}
                          >
                            <FileText class="h-3 w-3 shrink-0 text-[var(--rg-fg-muted)]" />
                            <span class="truncate text-[var(--rg-fg)]">{basename(f.path)}</span>
                            {#if dirname(f.path)}
                              <span class="text-[10px] text-[var(--rg-fg-muted)] truncate">
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
                                  class="flex h-5 w-5 items-center justify-center rounded hover:bg-[var(--rg-surface)] text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]"
                                  title="丢弃更改"
                                  onclick={(e) => { e.stopPropagation(); void discard(root, [f]); }}
                                >
                                  <Undo2 class="h-3 w-3" />
                                </button>
                                <button
                                  type="button"
                                  class="flex h-5 w-5 items-center justify-center rounded hover:bg-[var(--rg-surface)] text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]"
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
                      <div class="rg-scm-group-sticky w-full flex items-center gap-1 h-6 px-3 text-[10px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)] bg-[var(--rg-surface-2)]/92 backdrop-blur-md hover:bg-[var(--rg-surface)]/50 transition-colors">
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
                          class="flex h-5 w-5 items-center justify-center rounded opacity-0 group-hover/grp:opacity-100 hover:bg-[var(--rg-surface)] hover:text-red-400 transition-all"
                          title="永久删除全部未跟踪文件"
                          onclick={() => discard(root, s.untracked)}
                        >
                          <Undo2 class="h-3 w-3" />
                        </button>
                        <button
                          type="button"
                          class="flex h-5 w-5 items-center justify-center rounded opacity-0 group-hover/grp:opacity-100 hover:bg-[var(--rg-surface)] hover:text-[var(--rg-fg)] transition-all"
                          title="暂存全部未跟踪文件"
                          onclick={() => stage(root, s.untracked.map((f) => f.path))}
                        >
                          <Plus class="h-3 w-3" />
                        </button>
                        <span class="text-[var(--rg-fg)]">{s.untracked.length}</span>
                      </div>
                      {#if !isCollapsed(root, 'untracked')}
                        {#each s.untracked as f (f.path)}
                          <!-- Untracked rows are now click-to-diff like staged
                               and changes — `git_get_file_versions` with
                               cached=false treats a missing index blob as
                               empty original, rendering the entire file as
                               additions (matches VS Code's "U" file diff). -->
                          <div
                            class="group flex items-center gap-1.5 h-6 pl-6 pr-3 text-[11px] hover:bg-[var(--rg-surface)]/50 transition-colors cursor-pointer"
                            title="{f.path}（点击查看新文件 diff）"
                            role="button"
                            tabindex="0"
                            onclick={() => showDiff(root, f.path, false)}
                            onkeydown={(e) => e.target === e.currentTarget && e.key === 'Enter' && showDiff(root, f.path, false)}
                          >
                            <FileText class="h-3 w-3 shrink-0 text-[var(--rg-fg-muted)]" />
                            <span class="truncate text-[var(--rg-fg)]">{basename(f.path)}</span>
                            {#if dirname(f.path)}
                              <span class="text-[10px] text-[var(--rg-fg-muted)] truncate">
                                {dirname(f.path)}
                              </span>
                            {/if}
                            <span class="ml-auto flex items-center gap-0.5 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity">
                              <button
                                type="button"
                                class="flex h-5 w-5 items-center justify-center rounded hover:bg-[var(--rg-surface)] text-[var(--rg-fg-muted)] hover:text-red-400"
                                title="永久删除文件（不可撤销）"
                                onclick={(e) => { e.stopPropagation(); void discard(root, [f]); }}
                              >
                                <Undo2 class="h-3 w-3" />
                              </button>
                              <button
                                type="button"
                                class="flex h-5 w-5 items-center justify-center rounded hover:bg-[var(--rg-surface)] text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]"
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
                    <div class="px-3 py-2 text-[11px] text-[var(--rg-fg-muted)]">
                      工作区干净
                    </div>
                  {/if}
                {:else}
                  <div class="px-3 py-2 text-[11px] text-[var(--rg-fg-muted)]">加载中…</div>
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
          class="px-3 h-9 shrink-0 flex items-center justify-between gap-2 border-b border-[var(--rg-border)] bg-[var(--rg-surface)]/40"
        >
          <span class="text-[11px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)] shrink-0" title="带分支线 + merge 曲线的提交图谱">
            图谱
          </span>
          {#if repoRoots.length > 0}
            <select
              class="flex-1 min-w-0 text-[11px] px-1.5 py-0.5 rounded bg-[var(--rg-bg)] border border-[var(--rg-border)] text-[var(--rg-fg)] focus:outline-none focus:border-[var(--rg-accent)]/60"
              bind:value={selectedRepo}
              title={selectedRepo}
            >
              {#each repoRoots as root (root)}
                <option value={root}>{repoName(root)} — {collapseCwd(root)}</option>
              {/each}
            </select>
            <button
              type="button"
              class="flex h-6 w-6 shrink-0 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)] disabled:opacity-50 disabled:cursor-not-allowed"
              title="刷新（git pull --ff-only 后重载图谱；pull 失败仍刷新）"
              disabled={graphRefreshing || graphLoading}
              onclick={() => selectedRepo && void refreshGraphWithPull(selectedRepo)}
            >
              <RefreshCw class="h-3 w-3 {(graphRefreshing || graphLoading) ? 'animate-spin' : ''}" />
            </button>
          {/if}
        </div>

        <div class="flex-1 min-h-0" use:overlayScroll>
          {#if !selectedRepo}
            <div class="p-4 text-[12px] text-[var(--rg-fg-muted)] text-center">
              无 Git 仓库可显示
            </div>
          {:else if graphLoading && !graphInfo}
            <div class="p-4 text-[12px] text-[var(--rg-fg-muted)] text-center">加载中…</div>
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
              <GitGraph
                commits={graphInfo.commits}
                expandedHash={selectedCommitHash}
                expandedExtra={selectedCommitHash ? COMMIT_EXPAND_PX : 0}
              />
              <div class="flex-1 min-w-0">
                {#each graphInfo.commits as c (c.hash)}
                  <div
                    class="flex items-center gap-1.5 pr-3 cursor-pointer transition-colors {selectedCommitHash === c.hash
                      ? 'bg-[var(--rg-accent)]/15'
                      : 'hover:bg-[var(--rg-surface)]/40'}"
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
                        <span class="text-[10px] px-1 py-0.5 rounded bg-[var(--rg-surface)] text-[var(--rg-fg-muted)] shrink-0 font-mono" title={ref}>
                          {ref}
                        </span>
                      {/if}
                    {/each}
                    {#if hiddenRefs.length > 0}
                      <span
                        class="bg-[var(--rg-surface)] text-[var(--rg-fg-muted)] text-[10px] px-1 py-0.5 rounded font-mono shrink-0"
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
                      class="text-[12px] text-[var(--rg-fg)] flex-1 min-w-0 text-ellipsis w-0 whitespace-nowrap overflow-x-auto rg-msg-scroll"
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
                    <span class="text-[10px] font-mono text-[var(--rg-accent)]/80 shrink-0">
                      {c.hash.slice(0, 7)}
                    </span>
                    <span class="text-[10px] text-[var(--rg-fg-muted)] shrink-0 truncate max-w-[80px]">
                      {c.author}
                    </span>
                  </div>
                  <!-- Inline commit detail panel：展开时该 commit 行下方多腾 COMMIT_EXPAND_PX
                       高度（GitGraph 同步腾出），里面铺满 commit 元信息 + 文件列表。
                       点击文件进入 commit-vs-parent 的 diff（fileEditor 通过 commit 字段路由）。 -->
                  {#if selectedCommitHash === c.hash}
                    {@const bag = commitFilesFor(c.hash)}
                    <div
                      class="border-l-2 border-[var(--rg-accent)]/40 bg-[var(--rg-surface)]/30 overflow-hidden"
                      style="height: {COMMIT_EXPAND_PX}px"
                    >
                      <div class="h-full flex flex-col text-[11px]">
                        <div class="shrink-0 px-3 py-2 border-b border-[var(--rg-border)]/40">
                          <div class="text-[var(--rg-fg)] mb-1 break-words" title={c.subject}>
                            {c.subject}
                          </div>
                          <div class="flex items-center gap-2 text-[10px] text-[var(--rg-fg-muted)] font-mono">
                            <span>{c.hash.slice(0, 7)}</span>
                            <span>·</span>
                            <span class="truncate">{c.author}</span>
                            <span>·</span>
                            <span>{formatDate(c.date)}</span>
                          </div>
                        </div>
                        <div class="flex-1 min-h-0 overflow-y-auto rg-scroll-overlay py-1">
                          {#if bag?.loading}
                            <div class="px-3 py-2 text-[var(--rg-fg-muted)]">加载变动文件…</div>
                          {:else if bag?.error}
                            <div class="px-3 py-2 text-rose-300">无法读取：{bag.error}</div>
                          {:else if bag && bag.files.length === 0}
                            <div class="px-3 py-2 text-[var(--rg-fg-muted)]/70">无变动文件</div>
                          {:else if bag}
                            {#each bag.files as cf (cf.path)}
                              <button
                                type="button"
                                class="w-full flex items-center gap-2 px-3 py-1 text-left hover:bg-[var(--rg-accent)]/10 transition-colors"
                                title="查看 {cf.path} 在此 commit 的 diff"
                                onclick={() =>
                                  fileEditorStore.openDiffTab({
                                    repoRoot: selectedRepo!,
                                    path: cf.path,
                                    cached: false,
                                    commit: c.hash,
                                  })}
                              >
                                <span class="shrink-0 font-mono text-[10px] w-4 text-center {statusColor(cf.status)}">
                                  {statusLabel(cf.status)}
                                </span>
                                <span class="truncate text-[var(--rg-fg)]">{cf.path}</span>
                              </button>
                            {/each}
                          {/if}
                        </div>
                      </div>
                    </div>
                  {/if}
                {/each}
                <!-- T10：滚动 sentinel —— 进入视口时拉更多 commits。`use:rgGraphSentinel`
                     基于 IntersectionObserver；root 是上层 overlayScroll 容器，前端会
                     自动选最近 scrollable ancestor。 -->
                <div
                  class="h-6 flex items-center justify-center text-[10px] text-[var(--rg-fg-muted)]"
                  use:rgGraphSentinel={() => selectedRepo && loadMoreCommits(selectedRepo)}
                >
                  {#if loadingMoreCommits}
                    加载更早…
                  {:else if selectedRepo && noMoreCommits.has(selectedRepo)}
                    已到 git 历史末端
                  {/if}
                </div>
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
  /* T7：拖拽线贴在图谱面板的顶部边框上 —— 不再"浮空"。
     splitter 自身高度 1px 用 --rg-border 颜色，与图谱面板的顶部融为一体；
     ::before 把感应区集中放在下方（图谱内 6px），这样向下拖动就能直接命中，
     视觉上仍是"图谱顶部的一条线"，不再是"两面板间的悬浮线"。 */
  .scm-root :global(.splitpanes__splitter) {
    min-height: 1px;
    height: 1px;
    position: relative;
    background: var(--rg-border);
    transition: background-color 150ms ease;
  }
  .scm-root :global(.splitpanes__splitter::before) {
    content: '';
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
    bottom: -6px;
  }
  .scm-root :global(.splitpanes__splitter:hover) {
    background: color-mix(in oklab, var(--rg-accent) 50%, var(--rg-border));
  }
  /* svelte-splitpanes adds `splitpanes__splitter__active` to the splitter
     while it's being dragged (see node_modules/svelte-splitpanes/dist/Pane.svelte:89).
     `:active` is included as a fallback for the brief mousedown frame before
     the library's class lands. */
  .scm-root :global(.splitpanes__splitter:active),
  .scm-root :global(.splitpanes__splitter__active) {
    background: color-mix(in oklab, var(--rg-accent) 30%, transparent);
  }
  /* PaneDiffPill 跳转过来时给目标仓库一个短暂的高亮，让用户视觉锚定。
     1.5s 内淡出，不挡住 hover 状态。 */
  .scm-root :global(.scm-repo.rg-scm-flash) {
    animation: rg-scm-flash 1.5s ease-out;
  }
  @keyframes rg-scm-flash {
    0%, 25% { background: color-mix(in oklab, var(--rg-accent) 25%, transparent); }
    100%    { background: transparent; }
  }

  /* Sticky group sub-header — pinned right under the sticky repo header
     when the user scrolls within "更改" 面板。`top` 与 repo header 高度
     对齐（py-1.5 + h-6 内容 ≈ 29px）。`position: sticky` 不能用纯
     Tailwind 因为内联类还要拼运行时 hover/transition；这里集中给一个
     class 处理位置 + 层级，模板里用 `rg-scm-group-sticky` 引用。 */
  .scm-root :global(.rg-scm-group-sticky) {
    position: sticky;
    top: 29px;
    z-index: 20;
  }
  /* Per-row commit message: hide the native horizontal scrollbar that
     `overflow-x-auto` would render — overlayscrollbars per row would be
     overkill (one instance per visible commit). The Shift+wheel handler
     is the discoverable scroll affordance. */
  .scm-root :global(.rg-msg-scroll)::-webkit-scrollbar {
    display: none;
  }
  .scm-root :global(.rg-msg-scroll) {
    scrollbar-width: none;
  }
</style>
