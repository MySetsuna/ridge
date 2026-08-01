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
  isNotGitRepositoryError,
  isScmRepoKnownNonGit,
  markScmRepoNonGit,
  resetScmRepositoryDetection,
  clearScmQuerySingleFlights,
  getScmQueryDiagnostics,
  runScmQuerySingleFlight,
  setScmDirectoryContexts,
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
  resetScmRepositoryDetection();
  clearScmQuerySingleFlights();
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

  it('shares a non-Git result until the final directory context changes', () => {
    setScmRepoRoots(['/workspace/gone'], '/workspace', '/workspace/gone', ['/workspace']);
    setScmRepoStatus('/workspace/gone', fixtureStatus('/workspace/gone'));

    markScmRepoNonGit('/workspace/gone');
    expect(isScmRepoKnownNonGit('/workspace/gone')).toBe(true);
    expect(getScmCache().repoRoots).toEqual([]);
    expect(getScmCache().statuses['/workspace/gone']).toBeUndefined();

    // Same cwd discovery cannot resurrect the rejected root.
    setScmRepoRoots(['/workspace/gone'], '/workspace', '/workspace/gone', ['/workspace']);
    expect(getScmCache().repoRoots).toEqual([]);

    // Directory switch is the explicit re-probe boundary.
    setScmRepoRoots(['/workspace/gone'], '/other-workspace', '/workspace/gone', ['/other-workspace']);
    expect(isScmRepoKnownNonGit('/workspace/gone')).toBe(false);
    expect(getScmCache().repoRoots).toEqual(['/workspace/gone']);
  });

  it('classifies only explicit non-repository failures as negative detection', () => {
    expect(isNotGitRepositoryError('fatal: not a git repository')).toBe(true);
    expect(isNotGitRepositoryError('Not a git repo: C:/tmp')).toBe(true);
    expect(isNotGitRepositoryError('fatal: not inside a git work tree')).toBe(true);
    expect(isNotGitRepositoryError({ message: 'fatal: not a git repository' })).toBe(true);
    expect(isNotGitRepositoryError('git busy: concurrency permit timed out')).toBe(false);
    expect(isNotGitRepositoryError('superseded by newer request')).toBe(false);
  });

  it('keeps a negative root until its final pane owner leaves', () => {
    setScmDirectoryContexts('pane:a', ['/shared/repo/sub-a']);
    setScmDirectoryContexts('pane:b', ['/shared/repo/sub-b']);
    markScmRepoNonGit('/shared/repo');

    setScmDirectoryContexts('pane:a', ['/elsewhere']);
    expect(isScmRepoKnownNonGit('/shared/repo')).toBe(true);

    setScmDirectoryContexts('pane:b', ['/elsewhere']);
    expect(isScmRepoKnownNonGit('/shared/repo')).toBe(false);
  });

  it('single-flights 126 same-key reads and exposes exact diagnostics', async () => {
    let starts = 0;
    let release!: (value: string) => void;
    const deferred = new Promise<string>((resolve) => { release = resolve; });
    const calls = Array.from({ length: 126 }, () =>
      runScmQuerySingleFlight('branches', '/repo', () => {
        starts += 1;
        return deferred;
      })
    );

    await Promise.resolve();
    expect(starts).toBe(1);
    expect(getScmQueryDiagnostics()).toMatchObject({
      calls: 126,
      started: 1,
      joined: 125,
      inFlight: 1,
    });

    release('main');
    await expect(Promise.all(calls)).resolves.toEqual(Array(126).fill('main'));
    expect(getScmQueryDiagnostics()).toMatchObject({ completed: 1, inFlight: 0 });
  });

  it('single-flights equivalent Windows root spellings', async () => {
    let starts = 0;
    const first = runScmQuerySingleFlight('status', 'C:\\Repo\\', async () => {
      starts += 1;
      await Promise.resolve();
      return fixtureStatus('C:\\Repo');
    });
    const second = runScmQuerySingleFlight('status', 'c:/repo', async () => {
      starts += 1;
      return fixtureStatus('c:/repo');
    });

    expect(await second).toBe(await first);
    expect(starts).toBe(1);
  });

  it('holds branch and stash reads behind one in-flight status rejection', async () => {
    let releaseStatus!: () => void;
    const statusFailure = new Promise<ScmRepoStatus>((_resolve, reject) => {
      releaseStatus = () => reject(new Error('fatal: not a git repository'));
    });
    let branchesStarted = 0;
    let stashesStarted = 0;
    const status = runScmQuerySingleFlight('status', '/gone', () => statusFailure);
    const branches = runScmQuerySingleFlight('branches', '/gone', async () => {
      branchesStarted += 1;
      return [];
    });
    const stashes = runScmQuerySingleFlight('stashes', '/gone', async () => {
      stashesStarted += 1;
      return [];
    });

    await Promise.resolve();
    expect(branchesStarted).toBe(0);
    expect(stashesStarted).toBe(0);
    releaseStatus();
    await Promise.allSettled([status, branches, stashes]);
    expect(branchesStarted).toBe(0);
    expect(stashesStarted).toBe(0);
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
