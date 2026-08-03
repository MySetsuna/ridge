import { afterEach, describe, expect, it, vi } from 'vitest';
import type { DataProvider } from '$lib/transport';
import { createWsSidebarProvider } from './sidebarProvider';
import {
  fetchRemoteAgentHistory,
  fetchRemoteTeamRoster,
  REMOTE_SIDEBAR_STALE_TIME_MS,
  remoteQueryKeys,
} from './remoteQueries';

function makeProvider(overrides: Partial<DataProvider> = {}): DataProvider {
  return {
    getFileTree: vi.fn(async (path: string) => ({ path, children: [] })),
    getDirectoryChildren: vi.fn(),
    pathExists: vi.fn(),
    readFile: vi.fn(async () => 'content'),
    writeFile: vi.fn(async () => undefined),
    renamePath: vi.fn(),
    deletePath: vi.fn(),
    createFile: vi.fn(),
    createDirectory: vi.fn(),
    copyPath: vi.fn(),
    movePath: vi.fn(),
    revealInFileManager: vi.fn(),
    gitStatus: vi.fn(async () => ({ staged: [], unstaged: [], untracked: [], commits: [] })),
    gitStage: vi.fn(),
    gitUnstage: vi.fn(),
    gitCommit: vi.fn(),
    gitPull: vi.fn(),
    gitPush: vi.fn(),
    gitSync: vi.fn(),
    gitCheckout: vi.fn(),
    gitRevert: vi.fn(),
    gitCherryPick: vi.fn(),
    gitReset: vi.fn(),
    gitCreateTag: vi.fn(),
    gitDiscard: vi.fn(),
    gitCleanUntracked: vi.fn(),
    gitDiffFile: vi.fn(async () => ''),
    searchFiles: vi.fn(async () => []),
    ...overrides,
  } as unknown as DataProvider;
}

/** Small QueryClient-shaped test double; production uses TanStack Query. */
class TestQueryClient {
  private readonly cache = new Map<string, { promise: Promise<unknown>; expiresAt: number; staleTime: number }>();
  readonly invalidations: unknown[][] = [];

  fetchQuery<T>({
    queryKey,
    queryFn,
    staleTime = 0,
  }: {
    queryKey: readonly unknown[];
    queryFn: () => Promise<T>;
    staleTime?: number;
  }): Promise<T> {
    const key = JSON.stringify(queryKey);
    const hit = this.cache.get(key);
    // TanStack Query evaluates the current fetch's staleTime, not only the
    // value used by the request that populated the cache. An explicit refresh
    // passes staleTime=0 and must therefore bypass a previously fresh entry.
    if (hit && staleTime > 0 && hit.expiresAt > Date.now()) return hit.promise as Promise<T>;
    const promise = Promise.resolve().then(queryFn);
    this.cache.set(key, { promise, expiresAt: Date.now() + staleTime, staleTime });
    void promise.catch(() => {
      if (this.cache.get(key)?.promise === promise) this.cache.delete(key);
    });
    return promise;
  }

  invalidateQueries({ queryKey }: { queryKey: readonly unknown[] }): void {
    this.invalidations.push([...queryKey]);
  }
}

afterEach(() => vi.restoreAllMocks());

describe('remote sidebar query contract', () => {
  it('uses stable, session- and cwd-scoped keys', () => {
    expect(remoteQueryKeys.sidebarFiles(4, 'C:\\Repo\\', 'C:\\Repo\\', 1))
      .toEqual(['remote', 4, 'sidebar', '', '', '', 'files', 'c:/repo', 'c:/repo', 1]);
    expect(remoteQueryKeys.sidebarFiles(4, 'C:\\Repo', 'C:\\Repo', 1))
      .toEqual(remoteQueryKeys.sidebarFiles(4, 'C:\\Repo\\', 'C:\\Repo\\', 1));
    expect(remoteQueryKeys.sidebarGit(4, '/repo'))
      .not.toEqual(remoteQueryKeys.sidebarGit(5, '/repo'));
  });

  it('separates identical paths across workspace, pane, and branch scope', () => {
    const a = remoteQueryKeys.sidebarGit(4, '/repo', {
      workspaceId: 'ws-a', paneId: 'pane-a', branch: 'main',
    });
    const b = remoteQueryKeys.sidebarGit(4, '/repo', {
      workspaceId: 'ws-b', paneId: 'pane-a', branch: 'main',
    });
    const c = remoteQueryKeys.sidebarGit(4, '/repo', {
      workspaceId: 'ws-a', paneId: 'pane-a', branch: 'feature',
    });
    expect(a).not.toEqual(b);
    expect(a).not.toEqual(c);
  });

  it('single-flights and caches identical file requests across sidebar remounts', async () => {
    const client = new TestQueryClient();
    const gate: { resolve?: (value: { name: string; path: string; is_dir: boolean; children: [] }) => void } = {};
    const getFileTree = vi.fn((_path: string) => new Promise<{ name: string; path: string; is_dir: boolean; children: [] }>((resolve) => {
      gate.resolve = resolve;
    }));
    const dp = makeProvider({ getFileTree });
    const options = { queryClient: client, sessionId: 4, staleTime: REMOTE_SIDEBAR_STALE_TIME_MS };
    const firstProvider = createWsSidebarProvider('C:\\Repo', dp, options);
    const secondProvider = createWsSidebarProvider('C:\\Repo', dp, options);

    const first = firstProvider.listDir('');
    const second = secondProvider.listDir('');
    await Promise.resolve();
    expect(getFileTree).toHaveBeenCalledOnce();
    gate.resolve?.({ name: 'Repo', path: 'C:\\Repo', is_dir: true, children: [] });
    await expect(Promise.all([first, second])).resolves.toHaveLength(2);

    await firstProvider.listDir('');
    expect(getFileTree).toHaveBeenCalledOnce();
  });

  it('single-flights Git status but keeps cwd identities separate', async () => {
    const client = new TestQueryClient();
    const gitStatus = vi.fn(async () => ({
      current_branch: 'main',
      branches: ['main', 'feature/topic'],
      staged: [],
      unstaged: [],
      untracked: [],
      commits: [{ hash: 'head', msg: 'head commit', time: 'now', refs: ['head:', 'branch:main'] }],
    }));
    const dp = makeProvider({ gitStatus });
    const a = createWsSidebarProvider('/repo-a', dp, { queryClient: client, sessionId: 7 });
    const b = createWsSidebarProvider('/repo-b', dp, { queryClient: client, sessionId: 7 });

    const [first] = await Promise.all([a.gitStatus(), a.gitStatus(), b.gitStatus()]);
    expect(first.branches).toEqual(['main', 'feature/topic']);
    expect(first.commits[0]?.refs).toEqual(['head:', 'branch:main']);
    expect(gitStatus).toHaveBeenCalledTimes(2);
    expect(gitStatus).toHaveBeenNthCalledWith(1, '/repo-a', undefined);
    expect(gitStatus).toHaveBeenNthCalledWith(2, '/repo-b', undefined);
  });

  it('explicitly refreshes a cached directory without changing its Query key', async () => {
    const client = new TestQueryClient();
    const getFileTree = vi.fn(async (path: string) => ({
      name: 'repo',
      path,
      is_dir: true,
      children: [],
    }));
    const sidebar = createWsSidebarProvider('/repo', makeProvider({ getFileTree }), {
      queryClient: client,
      sessionId: 8,
      staleTime: 60_000,
    });

    await sidebar.listDir('');
    expect(getFileTree).toHaveBeenCalledOnce();
    await sidebar.listDir('');
    expect(getFileTree).toHaveBeenCalledOnce();
    await new Promise((resolve) => setTimeout(resolve, 2));
    await sidebar.refreshDir?.('');
    expect(getFileTree).toHaveBeenCalledTimes(2);
  });

  it('explicitly refreshes cached Git status while retaining non-Git fencing', async () => {
    const client = new TestQueryClient();
    const gitStatus = vi.fn(async () => ({
      is_git_repo: true,
      current_branch: 'main',
      staged: [],
      unstaged: [],
      untracked: [],
      commits: [],
    }));
    const sidebar = createWsSidebarProvider('/repo', makeProvider({ gitStatus }), {
      queryClient: client,
      sessionId: 8,
      staleTime: 60_000,
    });

    await sidebar.gitStatus();
    await sidebar.gitStatus();
    expect(gitStatus).toHaveBeenCalledOnce();
    await new Promise((resolve) => setTimeout(resolve, 2));
    await sidebar.refreshGit?.();
    expect(gitStatus).toHaveBeenCalledTimes(2);
  });

  it('caches lazy Graph history separately from the initial Git status', async () => {
    const client = new TestQueryClient();
    const gitGraph = vi.fn(async () => ({
      branches: ['main'],
      commits: [{ hash: 'head', msg: 'head', time: 'now', author: 'a', refs: ['head:'] }],
    }));
    const sidebar = createWsSidebarProvider('/repo', makeProvider({ gitGraph }), {
      queryClient: client,
      sessionId: 9,
    });

    const [first, second] = await Promise.all([sidebar.gitGraph?.(), sidebar.gitGraph?.()]);
    expect(first?.branches).toEqual(['main']);
    expect(second?.commits[0]?.hash).toBe('head');
    expect(gitGraph).toHaveBeenCalledOnce();
    await sidebar.gitGraph?.();
    expect(gitGraph).toHaveBeenCalledOnce();
  });

  it('keeps a clean Git repository visible when status has no files or commits', async () => {
    const dp = makeProvider({
      gitStatus: vi.fn(async () => ({
        is_git_repo: true,
        current_branch: 'main',
        staged: [],
        unstaged: [],
        untracked: [],
        commits: [],
      })),
    });
    const info = await createWsSidebarProvider('/clean-repo', dp).gitStatus();
    expect(info.isGitRepo).toBe(true);
    expect(info.files).toEqual([]);
    expect(info.currentBranch).toBe('main');
  });

  it('defaults successful legacy Git responses to repository=true', async () => {
    const dp = makeProvider({
      gitStatus: vi.fn(async () => ({ staged: [], unstaged: [], untracked: [], commits: [] })),
    });
    await expect(createWsSidebarProvider('/legacy-clean-repo', dp).gitStatus())
      .resolves.toMatchObject({ isGitRepo: true });
  });

  it('caches an explicit non-Git result for the provider root', async () => {
    const gitStatus = vi.fn(async () => { throw new Error('Not a git repo: /tmp/no-repo'); });
    const sidebar = createWsSidebarProvider('/tmp/no-repo', makeProvider({ gitStatus }));

    await expect(sidebar.gitStatus()).resolves.toMatchObject({ isGitRepo: false });
    await expect(sidebar.gitStatus()).resolves.toMatchObject({ isGitRepo: false });
    expect(gitStatus).toHaveBeenCalledOnce();
  });

  it('does not turn transport failures into a cached non-Git result', async () => {
    const gitStatus = vi.fn(async () => { throw new Error('socket timed out'); });
    const sidebar = createWsSidebarProvider('/tmp/repo', makeProvider({ gitStatus }));

    await expect(sidebar.gitStatus()).rejects.toThrow('socket timed out');
    await expect(sidebar.gitStatus()).rejects.toThrow('socket timed out');
    expect(gitStatus).toHaveBeenCalledTimes(2);
  });

  it('caches Agent roster and history across sidebar remounts', async () => {
    const client = new TestQueryClient();
    const topology = { roster: [], leaderId: null, edges: [] };
    const link = {
      getTeammateTopology: vi.fn(async () => topology),
      listHitlPending: vi.fn(async () => []),
      getOrchestrationHealth: vi.fn(async () => ({ suspendedAgents: 0, pendingHitl: 0 })),
      listAgentHistory: vi.fn(async () => []),
    } as unknown as import('@ridge/remote').RemoteLink;

    await Promise.all([
      fetchRemoteTeamRoster(link, client, 12, 'workspace-a'),
      fetchRemoteTeamRoster(link, client, 12, 'workspace-a'),
      fetchRemoteAgentHistory(link, client, 12),
      fetchRemoteAgentHistory(link, client, 12),
    ]);
    expect(link.getTeammateTopology).toHaveBeenCalledOnce();
    expect(link.listHitlPending).toHaveBeenCalledOnce();
    expect(link.getOrchestrationHealth).toHaveBeenCalledOnce();
    expect(link.listAgentHistory).toHaveBeenCalledOnce();

    await fetchRemoteTeamRoster(link, client, 12, 'workspace-a');
    await fetchRemoteAgentHistory(link, client, 12);
    expect(link.getTeammateTopology).toHaveBeenCalledOnce();
    expect(link.listAgentHistory).toHaveBeenCalledOnce();

    await fetchRemoteTeamRoster(link, client, 12, 'workspace-b');
    expect(link.getTeammateTopology).toHaveBeenCalledTimes(2);
  });

  it('delegates Git mutations and invalidates only affected Query keys', async () => {
    const client = new TestQueryClient();
    const gitCommit = vi.fn(async () => undefined);
    const gitPush = vi.fn(async () => undefined);
    const dp = makeProvider({ gitCommit, gitPush });
    const sidebar = createWsSidebarProvider('/repo', dp, {
      queryClient: client,
      sessionId: 9,
      workspaceId: 'ws-1',
      paneId: 'pane-1',
      branch: 'main',
    });

    await sidebar.gitCommit?.('message');
    await sidebar.gitPush?.(true);
    expect(gitCommit).toHaveBeenCalledWith('/repo', 'message', false);
    expect(gitPush).toHaveBeenCalledWith('/repo', true);
    expect(client.invalidations).toEqual([
      ['remote', 9, 'sidebar', 'ws-1', 'pane-1', 'main', 'git', '/repo'],
      ['remote', 9, 'sidebar', 'ws-1', 'pane-1', 'main', 'git-graph', '/repo'],
      ['remote', 9, 'sidebar', 'ws-1', 'pane-1', 'main', 'diff', '/repo'],
      ['remote', 9, 'sidebar', 'ws-1', 'pane-1', 'main', 'git', '/repo'],
      ['remote', 9, 'sidebar', 'ws-1', 'pane-1', 'main', 'git-graph', '/repo'],
    ]);
  });

  it('file writes invalidate file/diff/git/search snapshots without evicting unrelated keys', async () => {
    const client = new TestQueryClient();
    const dp = makeProvider({ writeFile: vi.fn(async () => undefined) });
    const sidebar = createWsSidebarProvider('/repo', dp, {
      queryClient: client,
      sessionId: 10,
      workspaceId: 'ws-1',
      paneId: 'pane-1',
      branch: 'main',
    });

    await sidebar.writeFile('/repo/src/app.ts', 'next');

    expect(client.invalidations).toEqual([
      ['remote', 10, 'sidebar', 'ws-1', 'pane-1', 'main', 'file', '/repo', '/repo/src/app.ts'],
      ['remote', 10, 'sidebar', 'ws-1', 'pane-1', 'main', 'diff', '/repo', '/repo/src/app.ts'],
      ['remote', 10, 'sidebar', 'ws-1', 'pane-1', 'main', 'git', '/repo'],
      ['remote', 10, 'sidebar', 'ws-1', 'pane-1', 'main', 'search', '/repo'],
    ]);
  });
});
