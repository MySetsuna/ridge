import { describe, expect, it, vi } from 'vitest';
import type { RemoteConnection } from '@ridge/remote';
import { WsDataProvider } from './ws';

function fakeConnection() {
  let onMessage: ((message: unknown) => void) | undefined;
  let onState: ((state: 'connecting' | 'connected' | 'disconnected' | 'error') => void) | undefined;
  let state: 'connecting' | 'connected' | 'disconnected' | 'error' = 'connected';
  const connection = {
    onMessage: vi.fn((fn: (message: unknown) => void) => {
      onMessage = fn;
      return () => { onMessage = undefined; };
    }),
    onStateChange: vi.fn((fn: (state: 'connecting' | 'connected' | 'disconnected' | 'error') => void) => {
      onState = fn;
      return () => { onState = undefined; };
    }),
    send: vi.fn(),
    state: () => state,
    emitMessage(message: unknown) { onMessage?.(message); },
    emitState(next: typeof state) { state=next; onState?.(next); },
  };
  return connection;
}

describe('WsDataProvider lifecycle', () => {
  it('rejects a new offline read/write immediately without queueing or replaying it', async () => {
    const connection=fakeConnection();const provider=new WsDataProvider(connection as unknown as RemoteConnection);
    connection.emitState('disconnected');await expect(provider.writeFile('/repo/a','unsaved')).rejects.toThrow('not connected');
    expect(connection.send).not.toHaveBeenCalled();connection.emitState('connected');expect(connection.send).not.toHaveBeenCalled();provider.dispose();
    await expect(provider.readFile('/repo/a')).rejects.toThrow('provider disposed');expect(connection.send).not.toHaveBeenCalled();
  });
  it('maps real Kernel search hits and keeps legacy response compatibility', async () => {
    const connection=fakeConnection();
    connection.send.mockImplementation((payload:{_reqId:number})=>queueMicrotask(()=>connection.emitMessage({_reqId:payload._reqId,_result:[{file:'/repo/a',line:3,column:5,content:'needle'},{path:'/repo/legacy',line:1,snippet:'old'}]})));
    const provider=new WsDataProvider(connection as unknown as RemoteConnection);
    await expect(provider.searchFiles('needle','/repo')).resolves.toEqual([{path:'/repo/a',line:3,column:5,snippet:'needle'},{path:'/repo/legacy',line:1,snippet:'old'}]);provider.dispose();
  });
  it('loads shared SCM status and sends exact reset and cleanup targets', async () => {
    const connection = fakeConnection();
    let untracked = [{ path: 'new.txt', status: '?' }];
    connection.send.mockImplementation((payload: { type?: string; _reqId?: number; method?: string }) => {
      if (payload.type !== 'data-request') return;
      queueMicrotask(() => connection.emitMessage({ type: 'data-result', _reqId: payload._reqId,
        _result: payload.method === 'get_scm_status'
          ? { repo_root:'/repo', current_branch: 'main', has_upstream: true, staged: [{ path: 'staged.txt', status: 'A' }], changes: [], untracked }
          : null }));
    });
    const provider = new WsDataProvider(connection as unknown as RemoteConnection);
    await expect(provider.gitStatus('/repo')).resolves.toMatchObject({
      repo_root:'/repo', is_git_repo: true, current_branch: 'main', staged: [{ name: 'staged.txt', status: 'A' }],
      unstaged: [], untracked: ['new.txt'], commits: [], branches: [],
    });
    await provider.gitReset('/repo', 'mixed', 'head');
    await provider.gitCleanUntracked('/repo');
    expect(connection.send).toHaveBeenCalledWith(expect.objectContaining({ method: 'git_reset', hash: 'head', mode: 'mixed' }));
    expect(connection.send).toHaveBeenCalledWith(expect.objectContaining({ method: 'git_clean_untracked', paths: ['new.txt'] }));
    connection.send.mockClear(); untracked = [];
    await provider.gitCleanUntracked('/repo');
    expect(connection.send).toHaveBeenCalledTimes(1);
    expect(connection.send).toHaveBeenCalledWith(expect.objectContaining({ method: 'get_scm_status' }));
    provider.dispose();
  });
  it('rejects all in-flight data queries as soon as transport disconnects', async () => {
    const connection = fakeConnection();
    const provider = new WsDataProvider(connection as unknown as RemoteConnection);
    const pending = provider.readFile('/repo/file.txt');

    connection.emitState('disconnected');

    await expect(pending).rejects.toThrow('transport disconnected');
    provider.dispose();
  });

  it('cleans the pending entry when send throws synchronously', async () => {
    const connection = fakeConnection();
    connection.send.mockImplementation(() => { throw new Error('socket closed'); });
    const provider = new WsDataProvider(connection as unknown as RemoteConnection);

    await expect(provider.gitStatus('/repo')).rejects.toThrow('socket closed');
    provider.dispose();
  });

  it('loads Git Graph through separate cancellable branch and history requests', async () => {
    const connection = fakeConnection();
    connection.send.mockImplementation((payload: { type?: string; _reqId?: number; method?: string }) => {
      if (payload.type !== 'data-request' || payload._reqId === undefined) return;
      queueMicrotask(() => connection.emitMessage({
        type: 'data-result',
        _reqId: payload._reqId,
        _result: payload.method === 'git_list_branches'
          ? [{ name: 'main' }]
          : [{ hash: 'head', subject: 'head', date: 'now', author: 'a', refs: ['head:'] }],
      }));
    });
    const provider = new WsDataProvider(connection as unknown as RemoteConnection);

    await expect(provider.gitGraph('/repo')).resolves.toMatchObject({
      branches: ['main'],
      commits: [{ hash: 'head', msg: 'head' }],
    });
    expect(connection.send).toHaveBeenCalledWith(expect.objectContaining({ method: 'git_list_branches' }));
    expect(connection.send).toHaveBeenCalledWith(expect.objectContaining({ method: 'get_git_commits_paginated' }));
    provider.dispose();
  });

  it('cancels non-signal requests when the provider is disposed', async () => {
    const connection = fakeConnection();
    const provider = new WsDataProvider(connection as unknown as RemoteConnection);
    const pending = provider.pathExists('/repo');
    const rejection = expect(pending).rejects.toThrow('provider disposed');

    provider.dispose();

    await rejection;
    expect(connection.send).toHaveBeenCalledWith({ type: 'data-cancel', _reqId: 1 });
  });

  it('honors an AbortSignal without waiting for the transport timeout', async () => {
    const connection = fakeConnection();
    const provider = new WsDataProvider(connection as unknown as RemoteConnection);
    const controller = new AbortController();
    const pending = provider.readFile('/repo/file.txt', controller.signal);

    controller.abort();

    await expect(pending).rejects.toMatchObject({ name: 'AbortError' });
    expect(connection.send).toHaveBeenCalledWith({ type: 'data-cancel', _reqId: 1 });
    provider.dispose();
  });

  it('honors an AbortSignal for a Git mutation as well', async () => {
    const connection = fakeConnection();
    const provider = new WsDataProvider(connection as unknown as RemoteConnection);
    const controller = new AbortController();
    const pending = provider.gitPush('/repo', false, controller.signal);

    controller.abort();

    await expect(pending).rejects.toMatchObject({ name: 'AbortError' });
    expect(connection.send).toHaveBeenCalledWith({ type: 'data-cancel', _reqId: 1 });
    provider.dispose();
  });

  it('cancels a timed-out request on the host instead of leaving remote work queued', async () => {
    vi.useFakeTimers();
    try {
      const connection = fakeConnection();
      const provider = new WsDataProvider(connection as unknown as RemoteConnection);
      const pending = provider.readFile('/repo/file.txt');
      const rejection = expect(pending).rejects.toThrow('timed out');
      await vi.advanceTimersByTimeAsync(10000);

      await rejection;
      expect(connection.send).toHaveBeenCalledWith({ type: 'data-cancel', _reqId: 1 });
      provider.dispose();
    } finally {
      vi.useRealTimers();
    }
  });
});
