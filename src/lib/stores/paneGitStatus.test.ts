import { beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

/**
 * Lock the contract that has been the source of repeated user reports
 * ("pane git pill shows on non-git cwd / shows mock data" + round 40
 * "must scan cwd-down, not cwd-up + multi-repo switcher"):
 *
 *   1. trackPaneGitStatus(pane, null) → store entry deleted
 *   2. trackPaneGitStatus(pane, non-git cwd) → backend returns []
 *      → store entry becomes null
 *   3. trackPaneGitStatus(pane, single-repo cwd) → store entry has
 *      branch + diff fields, availableRepos has 1 entry
 *   4. trackPaneGitStatus(pane, multi-repo cwd) → store entry has
 *      availableRepos with N entries, repoRoot defaults to first
 *   5. setPaneSelectedRepo switches repoRoot, availableRepos preserved
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

beforeEach(() => {
  mockInvoke.mockReset();
  vi.useFakeTimers();
});

/** Mock backend so any `find_git_repos_below(path)` returns the
 *  configured repos list for that path; get_scm_status / git_diff_summary
 *  return canonical fixture data per repo root. */
function mockBackend(reposByPath: Record<string, string[]>): void {
  mockInvoke.mockImplementation((cmd: string, args: unknown) => {
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
    mockBackend({ '/repo/sub': ['/repo'] });
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
      if (cmd === 'find_git_repos_below') {
        calls++;
        const path = (args as { path: string }).path;
        return Promise.resolve(path === '/code' ? ['/code/repo'] : []);
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

describe('cwd-down semantics + multi-repo switcher', () => {
  it('exposes single repo via availableRepos when cwd hosts exactly one', async () => {
    mockBackend({ '/code': ['/code/wind'] });
    mod.trackPaneGitStatus('p4', '/code');
    await vi.advanceTimersByTimeAsync(260);
    const info = get(mod.paneGitStatusStore).p4;
    expect(info?.repoRoot).toBe('/code/wind');
    expect(info?.availableRepos).toEqual(['/code/wind']);
  });

  it('exposes all repos and defaults to the first when cwd hosts multiple', async () => {
    mockBackend({ '/projects': ['/projects/a', '/projects/b', '/projects/c'] });
    mod.trackPaneGitStatus('p5', '/projects');
    await vi.advanceTimersByTimeAsync(260);
    const info = get(mod.paneGitStatusStore).p5;
    expect(info?.availableRepos).toEqual(['/projects/a', '/projects/b', '/projects/c']);
    expect(info?.repoRoot).toBe('/projects/a');
    expect(info?.branch).toBe('a');
  });

  it('setPaneSelectedRepo switches the active repo without losing availableRepos', async () => {
    mockBackend({ '/projects': ['/projects/a', '/projects/b'] });
    mod.trackPaneGitStatus('p6', '/projects');
    await vi.advanceTimersByTimeAsync(260);
    expect(get(mod.paneGitStatusStore).p6?.repoRoot).toBe('/projects/a');

    await mod.setPaneSelectedRepo('p6', '/projects/b');
    const info = get(mod.paneGitStatusStore).p6;
    expect(info?.repoRoot).toBe('/projects/b');
    expect(info?.branch).toBe('b');
    expect(info?.availableRepos).toEqual(['/projects/a', '/projects/b']);
  });

  it('drops a stale user pick when the underlying repos list changes', async () => {
    // Pane starts in /projects with [a, b]; user picks b. Then cwd
    // changes to /elsewhere where only [c] exists — the picked b is no
    // longer available, so we fall back to the first (c) without
    // surfacing a "missing repo" error.
    mockBackend({
      '/projects': ['/projects/a', '/projects/b'],
      '/elsewhere': ['/elsewhere/c'],
    });
    mod.trackPaneGitStatus('p7', '/projects');
    await vi.advanceTimersByTimeAsync(260);
    await mod.setPaneSelectedRepo('p7', '/projects/b');
    expect(get(mod.paneGitStatusStore).p7?.repoRoot).toBe('/projects/b');

    mod.trackPaneGitStatus('p7', '/elsewhere');
    await vi.advanceTimersByTimeAsync(260);
    const info = get(mod.paneGitStatusStore).p7;
    expect(info?.repoRoot).toBe('/elsewhere/c');
    expect(info?.availableRepos).toEqual(['/elsewhere/c']);
  });
});
