// src/lib/stores/paneGitStatus.ts
//
// Per-pane git summary (branch + diff counts). Fed by `find_git_repo_root`
// + `get_scm_status`, cached per
// repo root so multiple panes inside the same repo share a single fetch.
//
// A pane pill belongs to the cwd's Git root (itself or an ancestor). A
// non-Git container that merely has descendant repositories stays pill-free;
// SourceControl retains its separate workspace-wide discovery behavior.
//
// Refresh strategy: debounced on cwd change; lazy — the caller opts in by
// calling `trackPaneGitStatus(paneId, cwd)`. An explicit `invalidate()` hook
// lets the SCM sidebar tell us "status just changed" after stage/commit.

import { writable, get } from 'svelte/store';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { mapLimit, GIT_FANOUT_CONCURRENCY } from '$lib/utils/pLimit';
import { reportRepeatedError } from '$lib/utils/repeatedError';
import {
  isNotGitRepositoryError,
  invalidateScmQuery,
  isScmRepoKnownNonGit,
  markScmRepoNonGit,
  runScmQuerySingleFlight,
  setScmDirectoryContexts,
} from '$lib/stores/scmCache';

export interface PaneGitInfo {
  repoRoot: string;
  branch: string | null;
  /** Staged + unstaged added lines (best-effort from `get_scm_status`). */
  added: number;
  /** Staged + unstaged removed lines. */
  removed: number;
  /** Changed-file count (staged + unstaged + untracked). */
  dirtyFiles: number;
  /** ahead of upstream in commits, if known. */
  ahead: number;
  behind: number;
  /**
   * True iff the current branch tracks an upstream ref. Drives the pane pill's
   * amber "↑↓?" warning so the user notices `push` will need `-u` before
   * surprising them at the terminal. Defaults to `false` when the porcelain
   * line lacks an upstream segment (`## main` / `## main...`).
   */
  hasUpstream: boolean;
  /**
   * Compatibility DTO for the repo switcher. Pane root ownership exposes at
   * most one entry; SourceControl's workspace-wide discovery is independent.
   */
  availableRepos: string[];
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
  /** Backend `#[serde(default)]` — older snapshots may omit it. */
  has_upstream?: boolean;
}

const _store = writable<Record<string, PaneGitInfo | null>>({});
/** readonly from the outside — subscribe via this */
export const paneGitStatusStore = { subscribe: _store.subscribe };

/** Map pane-id → last-seen cwd so we can skip redundant fetches. */
const lastCwdByPane = new Map<string, string>();
/** repoRoot → in-flight promise so parallel pane requests coalesce. */
const inflightByRepo = new Map<string, Promise<RepoSnapshot | null>>();
/** Stable repoRoot → cached repo snapshot (pre-merge with availableRepos) + 取回时刻。 */
const cacheByRepo = new Map<string, { snap: RepoSnapshot | null; at: number }>();
/**
 * 快照复用窗口（ms）。同一 repo 在窗口内的重复请求直接吃缓存，不再 spawn git。
 *
 * 2026-07-26 多工作区 tab 卡死的放大器就在这里：每个 pane（**含隐藏工作区的**）
 * 都独立 track 自己的 git 状态，而 `resolveRepoSnapshot` 只**写**缓存从不**读**，
 * 于是 N 个 pane 指向同一个 repo 时就是 N 份 `get_scm_status`+`git_diff_summary`
 * ——而 `get_scm_status` 内部还要再开 3 个 git 进程。3 个 tab × 4 个 pane 同仓
 * ≈ 单轮刷新百余次 CreateProcess（Windows 每次 50–150ms），主线程 invoke 队列
 * 随即堵死，表现为「开两个以上 tab 就卡」。窗口取 1.5s：一轮突发（挂载潮/
 * watcher 刷新/心跳）收敛成每 repo 一次 git，用户手动操作后的刷新不受影响。
 */
const SNAPSHOT_TTL_MS = 1500;
/** Debounce timers per pane — a rapid cwd bounce won't trigger N fetches. */
const debounceTimers = new Map<string, ReturnType<typeof setTimeout>>();
/** User-chosen repo per pane when the pane's cwd hosts >1 repo. Cleared
 *  when the pane stops tracking. Survives across cwd changes as long as
 *  the chosen repo still appears in the new availableRepos list. */
/** Legacy selection slot kept for the switcher API. Root-owned pane resolution
 * clears it on every cwd resolution, so a descendant choice cannot persist. */
const selectedRepoByPane = new Map<string, string>();

interface GitDiffSummary {
  added: number;
  removed: number;
}

/** A single repo's resolved git data — common cache key, then merged with
 *  root-owned `availableRepos` when emitted to the store. */
// Root-owned pane DTO keeps this list at most one entry; SourceControl has
// separate workspace-wide discovery semantics.
type RepoSnapshot = Omit<PaneGitInfo, 'availableRepos'>;

async function resolveRepoSnapshot(
  repoRoot: string,
  slotBase?: string,
): Promise<RepoSnapshot | null> {
  if (isScmRepoKnownNonGit(repoRoot)) return null;
  // Coalesce concurrent calls for the same repoRoot. (The in-flight fetch is
  // registered under the INITIATING pane's supersede slot — if that pane
  // switches cwd mid-fetch the shared fetch dies and joiners see a transient
  // null; the next heartbeat/invalidate refetches. Accepted G1 trade-off.)
  const existing = inflightByRepo.get(repoRoot);
  if (existing) return existing;
  // 窗口内复用上一份快照：把「每 pane 一次 git」压回「每 repo 一次 git」。
  // 显式失效（invalidatePaneGitStatusForRepo）会先删条目，故不影响手动刷新时效。
  const cached = cacheByRepo.get(repoRoot);
  if (cached && Date.now() - cached.at < SNAPSHOT_TTL_MS) return cached.snap;
  const p = (async () => {
    try {
      // Run status + numstat in parallel — both hit git on the same repo
      // and the pill needs both to render. Distinct supersede slots per
      // command kind so the pair doesn't kill each other (G1 latest-win is
      // per (pane, kind); a NEWER refresh of the same pane kills both).
      const [s, diffSummary] = await Promise.all([
        runScmQuerySingleFlight('status', repoRoot, () =>
          invoke<ScmRepoStatus>('get_scm_status', {
            repoRoot,
            slot: `scm-status:${repoRoot}`,
          })
        ),
        runScmQuerySingleFlight('diff-summary', repoRoot, () =>
          invoke<GitDiffSummary>('git_diff_summary', {
            repoRoot,
            slot: slotBase ? `${slotBase}:diff` : `scm-diff:${repoRoot}`,
          })
        ).catch((error) => {
          if (isNotGitRepositoryError(error)) markPaneGitRepoNonGit(repoRoot);
          reportRepeatedError('git_diff_summary failed', error, 'warn');
          return { added: 0, removed: 0 };
        }),
      ]);
      if (isScmRepoKnownNonGit(repoRoot)) return null;
      const dirtyFiles = s.staged.length + s.changes.length + s.untracked.length;
      const snap: RepoSnapshot = {
        repoRoot: s.repo_root,
        branch: s.current_branch,
        added: diffSummary.added,
        removed: diffSummary.removed,
        dirtyFiles,
        ahead: s.ahead,
        behind: s.behind,
        hasUpstream: s.has_upstream ?? false,
      };
      cacheByRepo.set(repoRoot, { snap, at: Date.now() });
      return snap;
    } catch (err) {
      // G1: a superseded fetch is not "repo has no git data" — don't poison
      // the cache; the newer same-slot fetch (or next heartbeat) will land.
      const superseded = String(err).includes('superseded');
      if (!superseded) {
        cacheByRepo.set(repoRoot, { snap: null, at: Date.now() });
        if (isNotGitRepositoryError(err)) markPaneGitRepoNonGit(repoRoot);
        reportRepeatedError('get_scm_status failed', err);
      }
      return null;
    } finally {
      inflightByRepo.delete(repoRoot);
    }
  })();
  inflightByRepo.set(repoRoot, p);
  return p;
}

async function resolveInfoForPane(paneId: string, cwd: string): Promise<PaneGitInfo | null> {
  if (!isTauri() || !cwd) return null;
  // Pane branch ownership follows the cwd's Git ancestor. A workspace folder
  // that merely contains child repositories is not itself a repository, so
  // it must not surface a descendant branch pill.
  let repoRoot: string | null = null;
  try {
    repoRoot = await invoke<string | null>('find_git_repo_root', { path: cwd });
  } catch {
    return null;
  }
  if (!repoRoot || isScmRepoKnownNonGit(repoRoot)) return null;

  // A previous cwd-down selection must never leak into the new root-owned
  // contract. Keep the map clean so a later re-entry cannot resurrect it.
  selectedRepoByPane.delete(paneId);

  const snap = await resolveRepoSnapshot(repoRoot, `pane:${paneId}`);
  if (!snap) return null;
  return { ...snap, availableRepos: [repoRoot] };
}

/** Legacy switcher hook; root ownership remains backend-authoritative.
 *  and re-resolves the backend-owned root. */
/** Compatibility UI hook. Backend root remains authoritative, so a
 * descendant selection cannot replace the pane's cwd-owned pill. */
export async function setPaneSelectedRepo(paneId: string, repoRoot: string): Promise<void> {
  selectedRepoByPane.set(paneId, repoRoot);
  const cwd = lastCwdByPane.get(paneId);
  if (!cwd) return;
  const fresh = await resolveInfoForPane(paneId, cwd);
  _store.update((s) => ({ ...s, [paneId]: fresh }));
}

/**
 * Track a pane's cwd so its git info is kept fresh in the store. Call with
 * `cwd = null` to stop tracking (e.g. on pane close). Debounced 250ms so
 * cwd bounces during cd chains don't cause a burst of backend calls.
 */
export function trackPaneGitStatus(paneId: string, cwd: string | null): void {
  const prev = lastCwdByPane.get(paneId);
  // Normalize: prev is stored as '' when cwd was null (Map values are
  // strings). Compare both sides on the same shape so repeated null
  // calls early-return instead of churning store updates.
  const cwdNorm = cwd ?? '';
  if (prev === cwdNorm) return;
  const releasedRoots = setScmDirectoryContexts(`pane:${paneId}`, cwd ? [cwd] : []);
  for (const root of releasedRoots) cacheByRepo.delete(root);
  lastCwdByPane.set(paneId, cwdNorm);

  const existing = debounceTimers.get(paneId);
  if (existing) clearTimeout(existing);

  if (!cwd) {
    selectedRepoByPane.delete(paneId);
    _store.update((s) => {
      const next = { ...s };
      delete next[paneId];
      return next;
    });
    return;
  }

  debounceTimers.set(
    paneId,
    setTimeout(async () => {
      debounceTimers.delete(paneId);
      const info = await resolveInfoForPane(paneId, cwd);
      // G1 stale-overwrite guard: if the pane's cwd moved on while this fetch
      // was in flight, the newer fetch owns the store slot — drop this result.
      if (lastCwdByPane.get(paneId) !== cwd) return;
      _store.update((s) => ({ ...s, [paneId]: info }));
    }, 250)
  );
}

/**
 * Force a refetch for every pane whose cached repoRoot matches. Call after
 * staging / committing / pulling so the badge updates without waiting for
 * the next cwd change.
 */
export async function invalidatePaneGitStatusForRepo(repoRoot: string): Promise<void> {
  if (isScmRepoKnownNonGit(repoRoot)) return;
  cacheByRepo.delete(repoRoot);
  invalidateScmQuery('status', repoRoot);
  invalidateScmQuery('diff-summary', repoRoot);
  const all = get(_store);
  for (const [paneId, info] of Object.entries(all)) {
    // Invalidate panes whose root-owned repo matches. `availableRepos` remains
    // in the DTO for compatibility but contains at most that one root.
    if (
      info?.repoRoot === repoRoot ||
      info?.availableRepos?.includes(repoRoot)
    ) {
      const cwd = lastCwdByPane.get(paneId);
      if (cwd) {
        const fresh = await resolveInfoForPane(paneId, cwd);
        _store.update((s) => ({ ...s, [paneId]: fresh }));
      }
    }
  }
}

/** Close the pane-pill side of a confirmed non-Git root immediately. Branch
 * picker failures call this same path as status failures, so the 5-minute
 * heartbeat cannot resurrect the rejected root. */
export function markPaneGitRepoNonGit(repoRoot: string): void {
  markScmRepoNonGit(repoRoot);
  cacheByRepo.set(repoRoot, { snap: null, at: Date.now() });
  _store.update((state) =>
    Object.fromEntries(Object.entries(state).map(([paneId, info]) => {
      let next = info;
      if (info?.repoRoot === repoRoot) next = null;
      else if (info?.availableRepos.includes(repoRoot)) {
        next = { ...info, availableRepos: info.availableRepos.filter((root) => root !== repoRoot) };
      }
      return [paneId, next];
    }))
  );
}

/**
 * Refresh all currently-cached repos in the background. Called by the
 * 5-minute periodic timer so branch ahead/behind counts stay fresh even
 * when the user isn't doing SCM operations.
 */
async function refreshAllCachedRepos(): Promise<void> {
  const roots = Array.from(cacheByRepo.keys()).filter(
    (root) => !isScmRepoKnownNonGit(root)
  );
  // Limit concurrency: each invalidate cascades into `get_scm_status` +
  // `git_diff_summary` per pane, so a 5-minute heartbeat over 20 cached
  // repos would otherwise stampede git.exe on Windows.
  await mapLimit(roots, GIT_FANOUT_CONCURRENCY, (root) => invalidatePaneGitStatusForRepo(root));
}

// Background 5-minute heartbeat — keeps branch/diff counts accurate after
// external `git pull`, CI merges, or teammate operations the user didn't
// trigger from inside Ridge. Low cost: no-ops when no panes are tracked.
const PERIODIC_REFRESH_MS = 5 * 60 * 1000;
setInterval(() => { void refreshAllCachedRepos(); }, PERIODIC_REFRESH_MS);
