import { beforeEach, describe, expect, it } from 'vitest';
import { get } from 'svelte/store';
import {
  scmCacheStore,
  setScmRepoRoots,
  setScmRepoStatus,
  clearScmRepoStatus,
  getScmCache,
  shouldRefreshOnMount,
  setScmGraphInfo,
  clearScmGraphInfo,
  shouldRefreshGraphOnMount,
  setScmSelectedCommit,
  getScmSelectedCommit,
  type ScmRepoStatus,
  type GitRepoInfo,
} from './scmCache';

const fixtureStatus = (root: string): ScmRepoStatus => ({
  repo_root: root,
  current_branch: 'main',
  ahead: 0,
  behind: 0,
  staged: [],
  changes: [],
  untracked: [],
  has_upstream: true,
});

beforeEach(() => {
  // Wipe state between tests — the store is module-scope so it persists.
  setScmRepoRoots([], '', '');
});

describe('scmCacheStore', () => {
  it('starts empty after reset', () => {
    // beforeEach calls setScmRepoRoots([], …) which stamps lastDiscoverAt;
    // contract assertions are the empty roots + empty statuses, not the
    // timestamp.
    const c = getScmCache();
    expect(c.repoRoots).toEqual([]);
    expect(c.statuses).toEqual({});
  });

  it('setScmRepoRoots stamps lastDiscoverAt and writes signatures', () => {
    const before = Date.now();
    setScmRepoRoots(['/a', '/b'], 'cwd-sig', 'repo-sig');
    const c = get(scmCacheStore);
    expect(c.repoRoots).toEqual(['/a', '/b']);
    expect(c.lastCwdSignature).toBe('cwd-sig');
    expect(c.lastRepoSignature).toBe('repo-sig');
    expect(c.lastDiscoverAt).toBeGreaterThanOrEqual(before);
  });

  it('setScmRepoRoots drops statuses for repos no longer present', () => {
    setScmRepoRoots(['/a', '/b'], 's1', 'r1');
    setScmRepoStatus('/a', fixtureStatus('/a'));
    setScmRepoStatus('/b', fixtureStatus('/b'));
    expect(Object.keys(getScmCache().statuses).sort()).toEqual(['/a', '/b']);

    // /b removed → its status should be dropped, /a's preserved.
    setScmRepoRoots(['/a'], 's2', 'r2');
    const c = getScmCache();
    expect(c.repoRoots).toEqual(['/a']);
    expect(Object.keys(c.statuses)).toEqual(['/a']);
  });

  it('clearScmRepoStatus removes one entry without touching repoRoots', () => {
    setScmRepoRoots(['/a', '/b'], 's', 'r');
    setScmRepoStatus('/a', fixtureStatus('/a'));
    setScmRepoStatus('/b', fixtureStatus('/b'));
    clearScmRepoStatus('/a');
    const c = getScmCache();
    expect(c.repoRoots).toEqual(['/a', '/b']);
    expect(Object.keys(c.statuses)).toEqual(['/b']);
  });

  it('shouldRefreshOnMount: empty cache → true', () => {
    expect(shouldRefreshOnMount()).toBe(true);
  });

  it('shouldRefreshOnMount: fresh cache (within window) → false', () => {
    setScmRepoRoots(['/a'], 's', 'r');
    expect(shouldRefreshOnMount(30_000)).toBe(false);
  });

  it('shouldRefreshOnMount: stale cache → true', async () => {
    setScmRepoRoots(['/a'], 's', 'r');
    // Force the cache age past the window by passing a tiny maxAge.
    await new Promise((r) => setTimeout(r, 5));
    expect(shouldRefreshOnMount(1)).toBe(true);
  });
});

// ─── Graph info cache (round χ) ────────────────────────────────────────────

const fixtureGraph = (): GitRepoInfo => ({
  is_git_repo: true,
  commits: [
    {
      hash: 'abc1234',
      subject: 'init',
      author: 'dev',
      date: '1700000000',
      parents: [],
    },
  ],
  branches: ['main'],
  current_branch: 'main',
  diff: { files: [], total_additions: 0, total_deletions: 0, is_git_repo: true },
});

describe('scmCacheStore — graph info (round χ)', () => {
  beforeEach(() => {
    setScmRepoRoots([], '', '');
  });

  it('setScmGraphInfo stores the graph for a root', () => {
    setScmGraphInfo('/repo', fixtureGraph());
    const c = getScmCache();
    expect(c.graphInfos['/repo']).toBeDefined();
    expect(c.graphInfos['/repo'].commits).toHaveLength(1);
    expect(c.lastGraphLoadAt['/repo']).toBeGreaterThan(0);
  });

  it('clearScmGraphInfo removes the graph for a root', () => {
    setScmGraphInfo('/repo', fixtureGraph());
    clearScmGraphInfo('/repo');
    const c = getScmCache();
    expect(c.graphInfos['/repo']).toBeUndefined();
    expect(c.lastGraphLoadAt['/repo']).toBeUndefined();
  });

  it('setScmRepoRoots GCs graphInfos for removed roots', () => {
    setScmRepoRoots(['/a', '/b'], 's1', 'r1');
    setScmGraphInfo('/a', fixtureGraph());
    setScmGraphInfo('/b', fixtureGraph());
    // Remove /b from active roots.
    setScmRepoRoots(['/a'], 's2', 'r2');
    const c = getScmCache();
    expect(c.graphInfos['/a']).toBeDefined();
    expect(c.graphInfos['/b']).toBeUndefined();
  });

  it('shouldRefreshGraphOnMount returns true when no graph for root', () => {
    expect(shouldRefreshGraphOnMount('/missing')).toBe(true);
  });

  it('shouldRefreshGraphOnMount returns false when graph present and fresh', () => {
    setScmGraphInfo('/repo', fixtureGraph());
    expect(shouldRefreshGraphOnMount('/repo', 30_000)).toBe(false);
  });

  it('setScmSelectedCommit / getScmSelectedCommit roundtrip; setScmRepoRoots GCs it', () => {
    setScmRepoRoots(['/a', '/b'], 's1', 'r1');
    setScmSelectedCommit('/a', 'abc1234');
    setScmSelectedCommit('/b', 'def5678');

    expect(getScmSelectedCommit('/a')).toBe('abc1234');
    expect(getScmSelectedCommit('/b')).toBe('def5678');

    // GC: remove /b → its selection should be dropped.
    setScmRepoRoots(['/a'], 's2', 'r2');
    expect(getScmSelectedCommit('/a')).toBe('abc1234');
    expect(getScmSelectedCommit('/b')).toBe(''); // default fallback
  });
});
