import { expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { createCommuneLaunch, type LaunchRequest } from './communeLaunch';
import { RpcRemoteError } from '../../../packages/remote/src/shared/transport/types';

const selection = { profile: 'codex', cwd: '/project', runtimeEpoch: 'runtime' };
const receipt = (request: LaunchRequest) => ({ workspaceId: request.workspaceId, paneId: request.paneId });

it('does nothing until explicit submission and validates the exact receipt', async () => {
  const launch = createCommuneLaunch();
  expect(get(launch).pending).toBeUndefined();
  const submit = vi.fn(async request => receipt(request));
  const result = await launch.send(submit, selection);
  expect(result).toEqual(receipt(submit.mock.calls[0][0]));
  expect(submit.mock.calls[0][0]).toMatchObject({ entry: 'agent', cwd: '/project', collaboration: { profile: 'codex', arguments: [] } });
  expect(get(launch).pending).toBeUndefined();
});

it('keeps uncertain identities and configuration across subscribers and retries', async () => {
  const launch = createCommuneLaunch();
  await launch.send(async () => { throw new Error('connection lost'); }, selection);
  const original = get(launch).pending;
  const unsubscribe = launch.subscribe(() => {});
  unsubscribe();
  const submit = vi.fn(async request => receipt(request));
  await launch.send(submit, { profile: 'claude', cwd: '/elsewhere', runtimeEpoch: 'new-runtime' });
  expect(submit).toHaveBeenCalledExactlyOnceWith(original);
});

it('permits corrections after confirmed rejection under the same original identities and epoch', async () => {
  const launch = createCommuneLaunch();
  await launch.send(async () => { throw new RpcRemoteError('create', { code: -32602, message: 'invalid working directory' }); }, selection);
  const original = get(launch).pending;
  expect(get(launch).editable).toBe(true);
  const submit = vi.fn(async request => receipt(request));
  await launch.send(submit, { ...selection, cwd: '/fixed', runtimeEpoch: 'new-runtime' });
  expect(submit.mock.calls[0][0]).toEqual({ ...original, cwd: '/fixed' });
});

it('fences concurrent submits and preserves intent after a mismatched receipt', async () => {
  const launch = createCommuneLaunch();
  let finish!: (value: { workspaceId: string; paneId: string }) => void;
  const submit = vi.fn(() => new Promise<{ workspaceId: string; paneId: string }>(resolve => { finish = resolve; }));
  const pending = launch.send(submit, selection);
  await launch.send(submit, selection);
  expect(submit).toHaveBeenCalledOnce();
  finish({ workspaceId: 'another', paneId: 'another' });
  await pending;
  expect(get(launch).pending).toBeDefined();
  expect(get(launch).editable).toBe(false);
  expect(get(launch).error).toContain('different collaboration terminal identity');
});

it('freezes shared endpoint, protocol and credential reference after an unknown creation result', async () => {
  const launch = createCommuneLaunch();
  const shared = { cwd: '/project', runtimeEpoch: 'runtime', connect: 'ws://127.0.0.1:4500/', protocol: 'app-server' as const, bearerTokenFile: '/private/token' };
  await launch.send(async () => { throw Error('lost ACK'); }, shared);
  const original = get(launch).pending;
  expect(original?.collaboration).toEqual({ connect: shared.connect, protocol: shared.protocol, bearerTokenFile: shared.bearerTokenFile, arguments: [] });
  const submit = vi.fn(async request => receipt(request));
  await launch.send(submit, { ...shared, connect: 'ws://127.0.0.1:4600/' });
  expect(submit).toHaveBeenCalledExactlyOnceWith(original);
});
