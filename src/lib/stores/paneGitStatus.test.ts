import { beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

/**
 * Lock the contract that has been the source of repeated user reports
 * ("pane git pill shows on non-git cwd / shows mock data" + SCM root
 * ownership):
 *
 *   1. trackPaneGitStatus(pane, null) → store entry deleted
 *   2. trackPaneGitStatus(pane, non-git cwd) → backend returns []
 *      → store entry becomes null
 *   3. trackPaneGitStatus(pane, cwd inside a repo) → ancestor root owns pill
 *   4. trackPaneGitStatus(pane, parent of child repos) → no pill
 *   5. stale repo selections never cross a cwd/root boundary
 *
 * If any of these break, the pills will misrepresent state.
 */

vi.mock('@tauri-apps/api/core', () => ({
  isTauri: () => true,
  invoke: vi.fn(),
}));

const { invoke } = await import('@tauri-apps/api/core');
const mockInvoke = vi.mocked(invoke);

const mod = await import('./paneGitStatus');
const scm = await import('./scmCache');
const repeatedErrors = await import('$lib/utils/repeatedError');

beforeEach(() => {
  mockInvoke.mockReset();
  scm.resetScmRepositoryDetection();
  scm.clearScmQuerySingleFlights();
  repeatedErrors.clearRepeatedErrors();
  vi.useFakeTimers();
});

/** Mock backend for root ownership plus the legacy sidebar discovery call;
 * get_scm_status / git_diff_summary return canonical fixture data per root. */
function mockBackend(
  reposByPath: Record<string, string[]>,
  repoRootsByPath: Record<string, string | null> = {},
): void {
  mockInvoke.mockImplementation((cmd: string, args: unknown) => {
    if (cmd === 'find_git_repo_root') {
      return Promise.resolve(repoRootsByPath[(args as { path: string }).path] ?? null);
    }
    if (cmd === 'find_git_repos_below') {
      return Promise.resolve(reposByPath[(args as { path: string }).path] ?? []);
    }
    if (cmd === 'get_scm_status') {
      const root = (args as { repoRoot: string }).repoRoot;
      return Promise.resolve({
        repo_root: root,
        current_branch: root.split('/').pop(), // pretend branch matches dir name
        ahead: 0,
        behind: 0,
        staged: [],
        changes: [],
        untracked: [],
        has_upstream: true,
      });
    }
    if (cmd === 'git_diff_summary') {
      return Promise.resolve({ added: 0, removed: 0 });
    }
    return Promise.resolve(null);
  });
}

describe('trackPaneGitStatus null-cwd path', () => {
  it('clears the store entry when cwd is null', async () => {
    mockBackend({}, { '/repo/sub': '/repo' });
    mod.trackPaneGitStatus('p1', '/repo/sub');
    await vi.advanceTimersByTimeAsync(260);
    expect(get(mod.paneGitStatusStore).p1?.branch).toBe('repo');

    mod.trackPaneGitStatus('p1', null);
    expect(get(mod.paneGitStatusStore).p1).toBeUndefined();
  });

  it('returns null for cwd that has no git repo at or under it', async () => {
    mockBackend({ '/tmp/non-git': [] });
    mod.trackPaneGitStatus('p2', '/tmp/non-git');
    await vi.advanceTimersByTimeAsync(260);
    expect(get(mod.paneGitStatusStore).p2).toBeNull();
  });

  it('debounces rapid cwd bounces — only the last cwd resolves', async () => {
    let calls = 0;
    mockInvoke.mockImplementation((cmd: string, args: unknown) => {
      if (cmd === 'find_git_repo_root') {
        calls++;
        const path = (args as { path: string }).path;
        return Promise.resolve(path === '/code' ? '/code/repo' : null);
      }
      if (cmd === 'get_scm_status')
        return Promise.resolve({
          repo_root: '/code/repo',
          current_branch: 'final',
          ahead: 0,
          behind: 0,
          staged: [],
          changes: [],
          untracked: [],
          has_upstream: true,
        });
      if (cmd === 'git_diff_summary') return Promise.resolve({ added: 0, removed: 0 });
      return Promise.resolve(null);
    });
    mod.trackPaneGitStatus('p3', '/a');
    mod.trackPaneGitStatus('p3', '/b');
    mod.trackPaneGitStatus('p3', '/code');
    await vi.advanceTimersByTimeAsync(260);
    expect(calls).toBe(1);
    expect(get(mod.paneGitStatusStore).p3?.branch).toBe('final');
  });
});

describe('pane Git root ownership', () => {
  it('hides descendant repos when cwd is not inside a Git repo', async () => {
    mockBackend({ '/projects': ['/projects/a', '/projects/b'] }, { '/projects': null });
    mod.trackPaneGitStatus('p4', '/projects');
    await vi.advanceTimersByTimeAsync(260);
    expect(get(mod.paneGitStatusStore).p4).toBeNull();
  });

  it('resolves the ancestor Git root and exposes only it', async () => {
    mockBackend({}, { '/repo/src': '/repo' });
    mod.trackPaneGitStatus('p5', '/repo/src');
    await vi.advanceTimersByTimeAsync(260);
    const info = get(mod.paneGitStatusStore).p5;
    expect(info?.availableRepos).toEqual(['/repo']);
    expect(info?.repoRoot).toBe('/repo');
    expect(info?.branch).toBe('repo');
  });

  it('resolves a Git-root cwd directly', async () => {
    mockBackend({}, { '/repo': '/repo' });
    mod.trackPaneGitStatus('p6', '/repo');
    await vi.advanceTimersByTimeAsync(260);
    const info = get(mod.paneGitStatusStore).p6;
    expect(info?.repoRoot).toBe('/repo');
    expect(info?.availableRepos).toEqual(['/repo']);
  });

  it('clears the pane when cwd leaves the repository', async () => {
    mockBackend({}, { '/repo/src': '/repo', '/workspace': null });
    mod.trackPaneGitStatus('p7', '/repo/src');
    await vi.advanceTimersByTimeAsync(260);
    expect(get(mod.paneGitStatusStore).p7?.repoRoot).toBe('/repo');
    mod.trackPaneGitStatus('p7', '/workspace');
    await vi.advanceTimersByTimeAsync(260);
    expect(get(mod.paneGitStatusStore).p7).toBeNull();
  });
});

describe('non-Git repository detection cache', () => {
  it('stops 100 status invalidations after the first explicit non-repository failure', async () => {
    let scmCalls = 0;
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'find_git_repo_root') return Promise.resolve('/stale/repo');
      if (cmd === 'get_scm_status') {
        scmCalls++;
        return Promise.reject(new Error('fatal: not a git repository'));
      }
      if (cmd === 'git_diff_summary') return Promise.resolve({ added: 0, removed: 0 });
      return Promise.resolve(null);
    });

    mod.trackPaneGitStatus('non-git-cache', '/stale');
    await vi.advanceTimersByTimeAsync(260);
    expect(scmCalls).toBe(1);
    expect(get(mod.paneGitStatusStore)['non-git-cache']).toBeNull();

    await Promise.all(Array.from(
      { length: 100 },
      () => mod.invalidatePaneGitStatusForRepo('/stale/repo'),
    ));
    expect(scmCalls).toBe(1);
  });

  it('re-probes a rejected root after that pane switches directory', async () => {
    let scmCalls = 0;
    mockInvoke.mockImplementation((cmd: string, args: unknown) => {
      if (cmd === 'find_git_repo_root') return Promise.resolve('/shared/repo');
      if (cmd === 'get_scm_status') {
        scmCalls++;
        if (scmCalls === 1) {
          return Promise.reject(new Error('Not a git repo: /shared/repo'));
        }
        return Promise.resolve({
          repo_root: (args as { repoRoot: string }).repoRoot,
          current_branch: 'recovered',
          ahead: 0,
          behind: 0,
          staged: [],
          changes: [],
          untracked: [],
          has_upstream: true,
        });
      }
      if (cmd === 'git_diff_summary') return Promise.resolve({ added: 0, removed: 0 });
      return Promise.resolve(null);
    });

    mod.trackPaneGitStatus('non-git-cwd-switch', '/shared/repo/subdir');
    await vi.advanceTimersByTimeAsync(260);
    expect(scmCalls).toBe(1);

    mod.trackPaneGitStatus('non-git-cwd-switch', '/other');
    await vi.advanceTimersByTimeAsync(260);
    expect(scmCalls).toBe(2);
    expect(get(mod.paneGitStatusStore)['non-git-cwd-switch']?.branch).toBe('recovered');
  });

  it('keeps two panes suppressed until the final owner leaves the rejected root', async () => {
    let scmCalls = 0;
    mockInvoke.mockImplementation((cmd: string, args: unknown) => {
      if (cmd === 'find_git_repo_root') {
        return Promise.resolve(
          (args as { path: string }).path.startsWith('/owned') ? '/owned/repo' : null,
        );
      }
      if (cmd === 'get_scm_status') {
        scmCalls += 1;
        if (scmCalls === 1) return Promise.reject(new Error('fatal: not a git repository'));
        return Promise.resolve({
          repo_root: '/owned/repo',
          current_branch: 'recovered',
          ahead: 0,
          behind: 0,
          staged: [],
          changes: [],
          untracked: [],
          has_upstream: true,
        });
      }
      if (cmd === 'git_diff_summary') return Promise.resolve({ added: 0, removed: 0 });
      return Promise.resolve(null);
    });

    mod.trackPaneGitStatus('owner-a', '/owned/repo/a');
    mod.trackPaneGitStatus('owner-b', '/owned/repo/b');
    await vi.advanceTimersByTimeAsync(260);
    expect(scmCalls).toBe(1);

    mod.trackPaneGitStatus('owner-a', '/elsewhere/a');
    await vi.advanceTimersByTimeAsync(260);
    await Promise.all(Array.from(
      { length: 100 },
      () => mod.invalidatePaneGitStatusForRepo('/owned/repo'),
    ));
    expect(scmCalls).toBe(1);

    mod.trackPaneGitStatus('owner-b', '/elsewhere/b');
    await vi.advanceTimersByTimeAsync(260);
    mod.trackPaneGitStatus('owner-c', '/owned/repo/c');
    await vi.advanceTimersByTimeAsync(260);
    expect(scmCalls).toBe(2);
    expect(get(mod.paneGitStatusStore)['owner-c']?.branch).toBe('recovered');
  });

  it('keeps diff fallback usable while reporting its failure through aggregation', async () => {
    const warning = vi.spyOn(console, 'warn').mockImplementation(() => {});
    mockInvoke.mockImplementation((cmd: string, args: unknown) => {
      if (cmd === 'find_git_repo_root') return Promise.resolve('/diff/repo');
      if (cmd === 'get_scm_status') {
        return Promise.resolve({
          repo_root: (args as { repoRoot: string }).repoRoot,
          current_branch: 'main',
          ahead: 0,
          behind: 0,
          staged: [],
          changes: [],
          untracked: [],
          has_upstream: true,
        });
      }
      if (cmd === 'git_diff_summary') return Promise.reject(new Error('diff transport timeout'));
      return Promise.resolve(null);
    });

    mod.trackPaneGitStatus('diff-error', '/diff');
    await vi.advanceTimersByTimeAsync(260);
    expect(get(mod.paneGitStatusStore)['diff-error']).toMatchObject({ added: 0, removed: 0 });
    expect(warning).toHaveBeenCalledWith(
      'git_diff_summary failed',
      expect.objectContaining({ message: 'diff transport timeout' }),
    );
    warning.mockRestore();
  });
});

describe('多 pane / 多工作区 git 放大器（2026-07-26 卡死回归钉）', () => {
  // fake timer 冻结 Date.now：每个用例先越过快照复用窗口，免吃上一用例的缓存。
  beforeEach(async () => {
    await vi.advanceTimersByTimeAsync(2000);
  });

  it('同一 repo 的多个 pane 在窗口内只跑一次 git（每 repo 一次，而非每 pane 一次）', async () => {
    let scmCalls = 0;
    mockInvoke.mockImplementation((cmd: string, args: unknown) => {
      if (cmd === 'find_git_repo_root') {
        return Promise.resolve('/code/ridge');
      }
      if (cmd === 'get_scm_status') {
        scmCalls++;
        return Promise.resolve({
          repo_root: (args as { repoRoot: string }).repoRoot,
          current_branch: 'main',
          ahead: 0,
          behind: 0,
          staged: [],
          changes: [],
          untracked: [],
          has_upstream: true,
        });
      }
      if (cmd === 'git_diff_summary') return Promise.resolve({ added: 0, removed: 0 });
      return Promise.resolve(null);
    });

    // 10 个工作区 tab × 10 个 pane，全指向同一个 repo —— 旧实现 = 100 次 get_scm_status
    // （每次内部再开 3 个 git 进程），主线程 invoke 队列随即堵死。
    for (let ws = 0; ws < 10; ws++) {
      for (let p = 0; p < 10; p++) {
        mod.trackPaneGitStatus(`ws${ws}-pane${p}`, '/code');
      }
    }
    await vi.advanceTimersByTimeAsync(260);

    expect(scmCalls).toBe(1);
    // 每个 pane 仍拿到完整信息（复用同一快照，不是"只有第一个 pane 有数据"）。
    const all = get(mod.paneGitStatusStore);
    expect(all['ws0-pane0']?.branch).toBe('main');
    expect(all['ws9-pane9']?.branch).toBe('main');
  });

  it('显式失效后重新取（缓存不阻断手动/watcher 刷新）', async () => {
    let scmCalls = 0;
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'find_git_repo_root') return Promise.resolve('/code/ridge');
      if (cmd === 'get_scm_status') {
        scmCalls++;
        return Promise.resolve({
          repo_root: '/code/ridge',
          current_branch: `b${scmCalls}`,
          ahead: 0,
          behind: 0,
          staged: [],
          changes: [],
          untracked: [],
          has_upstream: true,
        });
      }
      if (cmd === 'git_diff_summary') return Promise.resolve({ added: 0, removed: 0 });
      return Promise.resolve(null);
    });

    mod.trackPaneGitStatus('inv1', '/code');
    await vi.advanceTimersByTimeAsync(260);
    expect(scmCalls).toBe(1);

    await mod.invalidatePaneGitStatusForRepo('/code/ridge');
    expect(scmCalls).toBe(2);
    expect(get(mod.paneGitStatusStore).inv1?.branch).toBe('b2');
  });
});
