import { describe, expect, it, vi } from 'vitest';
import type { RemoteLink } from '@ridge/remote';
import {
  REMOTE_ROSTER_STALE_TIME_MS,
  fetchRemoteAgentHistory,
  fetchRemoteQuery,
  fetchRemoteTeamRoster,
  remoteQueryKeys,
  remoteSessionId,
  remoteSidebarQueryPrefix,
} from './remoteQueries';

type FetchQuery = <T>(options: {
  queryKey: readonly unknown[];
  queryFn: (context?: { signal?: AbortSignal }) => Promise<T>;
  staleTime?: number;
}) => Promise<T>;

function makeFetchQuery() {
  const mock = vi.fn(({ queryFn }: {
    queryFn: (context?: { signal?: AbortSignal }) => Promise<unknown>;
  }) => queryFn());
  return mock as unknown as FetchQuery;
}

function link(): RemoteLink {
  return {
    getTeammateTopology: vi.fn(async () => ({ agents: [] })),
    listHitlPending: vi.fn(async () => []),
    getOrchestrationHealth: vi.fn(async () => ({ suspendedAgents: 0, pendingHitl: 0 })),
    listAgentHistory: vi.fn(async () => []),
  } as unknown as RemoteLink;
}

describe('remoteQueries', () => {
  it('keeps session ids stable per transport object', () => {
    const first = link();
    const second = link();
    expect(remoteSessionId(first)).toBe(remoteSessionId(first));
    expect(remoteSessionId(first)).not.toBe(remoteSessionId(second));
  });

  it('scopes sidebar keys by session, workspace, pane, branch, and normalized paths', () => {
    expect(remoteQueryKeys.sidebarFiles(3, 'C:\\Repo\\', 'C:\\Repo\\src', 2, {
      workspaceId: 'ws',
      paneId: 'pane',
      branch: 'main',
    })).toEqual([
      'remote', 3, 'sidebar', 'ws', 'pane', 'main', 'files', 'c:/repo', 'c:/repo/src', 2,
    ]);
    expect(remoteSidebarQueryPrefix(3, { workspaceId: 'ws' })).toEqual([
      'remote', 3, 'sidebar', 'ws', '', '',
    ]);
  });

  it('uses the query client and forwards the stale window', async () => {
    const fetchQuery = makeFetchQuery();
    const query = vi.fn(async () => 'fresh');
    await expect(fetchRemoteQuery({ fetchQuery }, ['key'], query, 1234)).resolves.toBe('fresh');
    expect(fetchQuery).toHaveBeenCalledWith(expect.objectContaining({
      queryKey: ['key'],
      staleTime: 1234,
    }));
    expect(query).toHaveBeenCalledOnce();
  });

  it('cancels one observer without cancelling the shared query work', async () => {
    let resolve!: (value: string) => void;
    const work = new Promise<string>((done) => { resolve = done; });
    const controller = new AbortController();
    const observed = fetchRemoteQuery(undefined, ['key'], () => work, 0, controller.signal);
    controller.abort('view destroyed');
    await expect(observed).rejects.toBe('view destroyed');
    resolve('late result');
    await expect(work).resolves.toBe('late result');
  });

  it('keeps roster results typed when health RPC is unavailable', async () => {
    const remote = link();
    vi.mocked(remote.getOrchestrationHealth).mockRejectedValueOnce(new Error('old host'));
    const fetchQuery = makeFetchQuery();
    await expect(fetchRemoteTeamRoster(remote, {
      fetchQuery,
    }, 7, 'workspace-a')).resolves.toEqual({
      topology: { agents: [] },
      pending: [],
      health: { suspendedAgents: 0, pendingHitl: 0 },
    });
    expect(fetchQuery).toHaveBeenCalledWith(expect.objectContaining({
      queryKey: remoteQueryKeys.teamRoster(7, 'workspace-a'),
      staleTime: REMOTE_ROSTER_STALE_TIME_MS,
    }));
  });

  it('routes history through the host-wide query key and limit', async () => {
    const remote = link();
    const history = [{ agent: 'Codex', sessionId: 's1' }];
    vi.mocked(remote.listAgentHistory).mockResolvedValueOnce(history as never);
    const fetchQuery = makeFetchQuery();
    await expect(fetchRemoteAgentHistory(remote, { fetchQuery }, 9, 12)).resolves.toEqual(history);
    expect(remote.listAgentHistory).toHaveBeenCalledWith(12);
    expect(fetchQuery).toHaveBeenCalledWith(expect.objectContaining({
      queryKey: remoteQueryKeys.agentHistory(9, 12),
    }));
  });
});
