// src/lib/stores/paneGitStatus.ts
//
// Per-pane git summary (branch + diff counts). Fed by `find_git_repos_below`
// (depth=1: cwd self + immediate children) + `get_scm_status`, cached per
// repo root so multiple panes inside the same repo share a single fetch.
//
// **Round 40 semantics change**: previously walked UP the directory tree
// via `find_git_repo_root`, which is git's standard "you're in a repo if
// any ancestor has .git" rule. The user explicitly wanted "cwd is the
// container" — only repos discovered at cwd or directly under it count.
// Net effect: if the user opens Ridge in `~/Downloads` (non-git, no .git
// children), no pill renders even if `~/.git` happens to exist somewhere
// far above. If they open it in `~/code` and `~/code/{a,b,c}` are each
// repos, all three are surfaced and a switcher renders next to the pill.
//
// Refresh strategy: debounced on cwd change; lazy — the caller opts in by
// calling `trackPaneGitStatus(paneId, cwd)`. An explicit `invalidate()` hook
// lets the SCM sidebar tell us "status just changed" after stage/commit.

import { writable, get } from 'svelte/store';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { mapLimit, GIT_FANOUT_CONCURRENCY } from '$lib/utils/pLimit';

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
   * Every git repo found at the pane's cwd or directly under it (depth=1
   * `find_git_repos_below` scan). When length > 1, the UI renders a
   * `PaneRepoSwitcher` left of the branch pill so the user can pick which
   * repo's data the rest of the pills reflect. `repoRoot` above is always
   * one of these.
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
/** Stable repoRoot → cached repo snapshot (pre-merge with availableRepos). */
const cacheByRepo = new Map<string, RepoSnapshot | null>();
/** Debounce timers per pane — a rapid cwd bounce won't trigger N fetches. */
const debounceTimers = new Map<string, ReturnType<typeof setTimeout>>();
/** User-chosen repo per pane when the pane's cwd hosts >1 repo. Cleared
 *  when the pane stops tracking. Survives across cwd changes as long as
 *  the chosen repo still appears in the new availableRepos list. */
const selectedRepoByPane = new Map<string, string>();

interface GitDiffSummary {
  added: number;
  removed: number;
}

/** A single repo's resolved git data — common cache key, then merged with
 *  per-pane `availableRepos` when emitted to the store. */
type RepoSnapshot = Omit<PaneGitInfo, 'availableRepos'>;

async function resolveRepoSnapshot(repoRoot: string): Promise<RepoSnapshot | null> {
  // Coalesce concurrent calls for the same repoRoot.
  const existing = inflightByRepo.get(repoRoot);
  if (existing) return existing;
  const p = (async () => {
    try {
      // Run status + numstat in parallel — both hit git on the same repo
      // and the pill needs both to render.
      const [s, diffSummary] = await Promise.all([
        invoke<ScmRepoStatus>('get_scm_status', { repoRoot }),
        invoke<GitDiffSummary>('git_diff_summary', { repoRoot }).catch(() => ({
          added: 0,
          removed: 0,
        })),
      ]);
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
      cacheByRepo.set(repoRoot, snap);
      return snap;
    } catch {
      cacheByRepo.set(repoRoot, null);
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
  // **Round 40 — cwd-down semantics**: scan cwd self + depth-1 children
  // for any `.git/` markers. If empty, this pane is genuinely "outside
  // any git repo" from the user's mental model and the pill must hide.
  let repos: string[] = [];
  try {
    repos = await invoke<string[]>('find_git_repos_below', {
      path: cwd,
      maxDepth: 1,
    });
  } catch {
    return null;
  }
  if (repos.length === 0) return null;

  // Pick which repo this pane should currently surface. User selection
  // (via PaneRepoSwitcher) wins if it's still in the discovered list;
  // otherwise default to the first (alphabetical, since the backend
  // already sort+deduped).
  const userPick = selectedRepoByPane.get(paneId);
  const repoRoot = userPick && repos.includes(userPick) ? userPick : repos[0];
  if (userPick && !repos.includes(userPick)) {
    selectedRepoByPane.delete(paneId);
  }

  const snap = await resolveRepoSnapshot(repoRoot);
  if (!snap) return null;
  return { ...snap, availableRepos: repos };
}

/** UI hook: switch which repo a pane's pill reflects. Triggers a re-resolve
 *  using the cached snapshot for the picked repo (no backend roundtrip
 *  needed if it was cached during the same window). */
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
  cacheByRepo.delete(repoRoot);
  const all = get(_store);
  for (const [paneId, info] of Object.entries(all)) {
    // After round 40 a pane has `availableRepos` — invalidate any pane
    // whose currently-selected repo OR any of its discovered repos
    // matches (a stage in repo B might also be visible from a pane
    // showing repo A if both are siblings under the same cwd).
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

/**
 * Refresh all currently-cached repos in the background. Called by the
 * 5-minute periodic timer so branch ahead/behind counts stay fresh even
 * when the user isn't doing SCM operations.
 */
async function refreshAllCachedRepos(): Promise<void> {
  const roots = Array.from(cacheByRepo.keys());
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
