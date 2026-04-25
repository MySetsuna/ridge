// src/lib/stores/scmCache.ts
//
// Persistent (module-scope) cache for the Source Control panel's discovered
// repos + their status snapshots. SourceControl.svelte gets unmounted every
// time the user switches off the `git` sidebar tab; without a cache its
// `repoRoots` + `statuses` $state would re-init to empty and trigger a
// full re-discovery + per-repo `get_scm_status` round-trip on every
// re-mount. With this cache, switching back to SCM is instant — the panel
// hydrates from the cached snapshot, then schedules a background refresh
// in case anything moved while the tab was hidden.
//
// **Important**: this is the MVP for the "切换到源代码管理 tab 不要每次重
// 新加载" ask (ε). A real filesystem-watcher (`notify` crate) layer can
// replace the periodic background refresh later — the store shape stays
// the same, only the invalidation source changes.
//
// Round χ: adds graph-info caching (GitRepoInfo per repo root) so the
// expensive `get_git_info_with_cwd` IPC call (git2 log walk) is also
// served from cache on tab remount instead of being re-fired every time.

import { writable, get } from 'svelte/store';

export interface ScmFile {
  path: string;
  status: string;
  group: string;
  additions?: number;
  deletions?: number;
}

export interface ScmRepoStatus {
  repo_root: string;
  current_branch: string | null;
  ahead: number;
  behind: number;
  staged: ScmFile[];
  changes: ScmFile[];
  untracked: ScmFile[];
  has_upstream?: boolean;
}

// Shared GitRepoInfo type — sourced here so SourceControl.svelte can import
// it without a circular dependency (round χ).
export interface CommitNode {
  hash: string;
  subject: string;
  author: string;
  date: string;
  parents: string[];
  branch?: string;
  /** Optional ref decorations: `head:`, `branch:main`, `tag:v1.0`. */
  refs?: string[];
}
export interface DiffFile {
  path: string;
  additions: number;
  deletions: number;
  status: string;
}
export interface GitRepoInfo {
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

export interface ScmCacheState {
  /** Discovered git repo roots — sorted, deduped. */
  repoRoots: string[];
  /** Per-repo last-known status snapshot. Key = repo root. */
  statuses: Record<string, ScmRepoStatus>;
  /** Per-repo last-known git graph info. Key = repo root. */
  graphInfos: Record<string, GitRepoInfo>;
  /** Wall-clock millis when each graphInfo was last fetched. Key = repo root. */
  lastGraphLoadAt: Record<string, number>;
  /** Per-repo selected commit hash in the graph view. Key = repo root. */
  selectedCommitHashByRepo: Record<string, string>;
  /** The repo currently selected in the SCM panel's repo picker.
   *  Allows callers outside SourceControl (e.g. the git-graph context menu
   *  in +page.svelte) to target the same repo the user is looking at. */
  selectedScmRepo: string;
  /** Pipe-joined sorted unique cwds last scanned — used to skip
   *  re-discovery when the cwd set hasn't changed since last visit. */
  lastCwdSignature: string;
  /** Pipe-joined sorted repo roots last computed — used to detect
   *  "discovery returned the same set" so the panel can skip status
   *  re-fetches when nothing structural has shifted. */
  lastRepoSignature: string;
  /** Wall-clock millis when the cache was last successfully populated.
   *  Lets the panel decide whether to schedule a background refresh
   *  on remount (e.g. >30s old → refresh, fresher → trust cache). */
  lastDiscoverAt: number;
}

const _store = writable<ScmCacheState>({
  repoRoots: [],
  statuses: {},
  graphInfos: {},
  lastGraphLoadAt: {},
  selectedCommitHashByRepo: {},
  selectedScmRepo: '',
  lastCwdSignature: '',
  lastRepoSignature: '',
  lastDiscoverAt: 0,
});

/** Read-only subscription handle for components. */
export const scmCacheStore = { subscribe: _store.subscribe };

/** Imperative cache writers. SourceControl owns the discover/refresh
 *  logic itself for now (heavy interaction with its UI state); this
 *  module only stores results. */
export function setScmRepoRoots(
  repoRoots: string[],
  cwdSignature: string,
  repoSignature: string
): void {
  _store.update((s) => ({
    ...s,
    repoRoots,
    lastCwdSignature: cwdSignature,
    lastRepoSignature: repoSignature,
    lastDiscoverAt: Date.now(),
    // Drop snapshots for repos no longer present so memory doesn't
    // accumulate forever as the user opens/closes folders.
    statuses: Object.fromEntries(
      Object.entries(s.statuses).filter(([root]) => repoRoots.includes(root))
    ),
    graphInfos: Object.fromEntries(
      Object.entries(s.graphInfos).filter(([root]) => repoRoots.includes(root))
    ),
    lastGraphLoadAt: Object.fromEntries(
      Object.entries(s.lastGraphLoadAt).filter(([root]) => repoRoots.includes(root))
    ),
    selectedCommitHashByRepo: Object.fromEntries(
      Object.entries(s.selectedCommitHashByRepo).filter(([root]) => repoRoots.includes(root))
    ),
  }));
}

export function setScmRepoStatus(repoRoot: string, status: ScmRepoStatus): void {
  _store.update((s) => ({
    ...s,
    statuses: { ...s.statuses, [repoRoot]: status },
  }));
}

export function clearScmRepoStatus(repoRoot: string): void {
  _store.update((s) => {
    const next = { ...s.statuses };
    delete next[repoRoot];
    return { ...s, statuses: next };
  });
}

// ─── Graph info cache (round χ) ───────────────────────────────────────────

export function setScmGraphInfo(repoRoot: string, info: GitRepoInfo): void {
  _store.update((s) => ({
    ...s,
    graphInfos: { ...s.graphInfos, [repoRoot]: info },
    lastGraphLoadAt: { ...s.lastGraphLoadAt, [repoRoot]: Date.now() },
  }));
}

export function clearScmGraphInfo(repoRoot: string): void {
  _store.update((s) => {
    const graphInfos = { ...s.graphInfos };
    const lastGraphLoadAt = { ...s.lastGraphLoadAt };
    delete graphInfos[repoRoot];
    delete lastGraphLoadAt[repoRoot];
    return { ...s, graphInfos, lastGraphLoadAt };
  });
}

/**
 * Decide whether a remount should trigger a full graph load for the given
 * repo: no cached graph, or cache older than `maxAgeMs` (default 30s).
 */
export function shouldRefreshGraphOnMount(repoRoot: string, maxAgeMs = 30_000): boolean {
  const c = getScmCache();
  if (!c.graphInfos[repoRoot]) return true;
  return Date.now() - (c.lastGraphLoadAt[repoRoot] ?? 0) > maxAgeMs;
}

// ─── Selected commit hash per repo ────────────────────────────────────────

export function setScmSelectedCommit(repoRoot: string, hash: string): void {
  _store.update((s) => ({
    ...s,
    selectedCommitHashByRepo: { ...s.selectedCommitHashByRepo, [repoRoot]: hash },
  }));
}

export function getScmSelectedCommit(repoRoot: string): string {
  return get(_store).selectedCommitHashByRepo[repoRoot] ?? '';
}

// ─── SCM panel's active repo selection ────────────────────────────────────

/** Called by SourceControl whenever its `selectedRepo` changes, so external
 *  callers (e.g. git-graph context menu in +page.svelte) know which repo to
 *  target without reaching into component-local state. */
export function setScmSelectedRepo(repoRoot: string): void {
  _store.update((s) => ({ ...s, selectedScmRepo: repoRoot }));
}

export function getScmSelectedRepo(): string {
  return get(_store).selectedScmRepo;
}

/** Snapshot accessor for non-reactive callers (effects that read once). */
export function getScmCache(): ScmCacheState {
  return get(_store);
}

/**
 * Decide whether a remount should trigger a background refresh: cache is
 * empty, or older than `maxAgeMs` (default 30s — long enough that a quick
 * tab toggle is instant, short enough that real changes during a longer
 * absence don't go unnoticed).
 */
export function shouldRefreshOnMount(maxAgeMs = 30_000): boolean {
  const c = getScmCache();
  if (c.repoRoots.length === 0) return true;
  return Date.now() - c.lastDiscoverAt > maxAgeMs;
}
