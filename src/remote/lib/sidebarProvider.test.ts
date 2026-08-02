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
  private readonly cache = new Map<string, { promise: Promise<unknown>; expiresAt: number }>();
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
    if (hit && hit.expiresAt > Date.now()) return hit.promise as Promise<T>;
    const promise = Promise.resolve().then(queryFn);
    this.cache.set(key, { promise, expiresAt: Date.now() + staleTime });
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
      .toEqual(['remote', 4, 'sidebar', 'files', 'c:/repo', 'c:/repo', 1]);
    expect(remoteQueryKeys.sidebarFiles(4, 'C:\\Repo', 'C:\\Repo', 1))
      .toEqual(remoteQueryKeys.sidebarFiles(4, 'C:\\Repo\\', 'C:\\Repo\\', 1));
    expect(remoteQueryKeys.sidebarGit(4, '/repo'))
      .not.toEqual(remoteQueryKeys.sidebarGit(5, '/repo'));
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
    const gitStatus = vi.fn(async () => ({ staged: [], unstaged: [], untracked: [], commits: [] }));
    const dp = makeProvider({ gitStatus });
    const a = createWsSidebarProvider('/repo-a', dp, { queryClient: client, sessionId: 7 });
    const b = createWsSidebarProvider('/repo-b', dp, { queryClient: client, sessionId: 7 });

    await Promise.all([a.gitStatus(), a.gitStatus(), b.gitStatus()]);
    expect(gitStatus).toHaveBeenCalledTimes(2);
    expect(gitStatus).toHaveBeenNthCalledWith(1, '/repo-a');
    expect(gitStatus).toHaveBeenNthCalledWith(2, '/repo-b');
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

  it('delegates Git mutations and invalidates only the session sidebar prefix', async () => {
    const client = new TestQueryClient();
    const gitCommit = vi.fn(async () => undefined);
    const gitPush = vi.fn(async () => undefined);
    const dp = makeProvider({ gitCommit, gitPush });
    const sidebar = createWsSidebarProvider('/repo', dp, { queryClient: client, sessionId: 9 });

    await sidebar.gitCommit?.('message');
    await sidebar.gitPush?.(true);
    expect(gitCommit).toHaveBeenCalledWith('/repo', 'message', false);
    expect(gitPush).toHaveBeenCalledWith('/repo', true);
    expect(client.invalidations).toEqual([
      ['remote', 9, 'sidebar'],
      ['remote', 9, 'sidebar'],
    ]);
  });
});
