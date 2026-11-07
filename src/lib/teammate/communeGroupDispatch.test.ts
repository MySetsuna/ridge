import { expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { communeGroupDispatch } from './communeGroupDispatch';
import type { CommuneGroupDispatchRequest, CommuneGroupTask } from '../../../packages/remote/src/shared/transport/wsRemote';

const request = (message = 'original'): CommuneGroupDispatchRequest => ({
  groupScope: 'host', groupId: 'group', groupRevision: 3, groupAction: 'broadcast', message,
  idempotency_key: 'one-intent', targets: [{ agentId: 'member', generation: 2, lease: 'lease' }],
});
const receipt = (submitted: CommuneGroupDispatchRequest): CommuneGroupTask => ({
  groupTaskId: 'task', groupScope: submitted.groupScope, workspaceId: submitted.workspaceId,
  groupId: submitted.groupId, groupRevision: submitted.groupRevision, groupName: 'Group', action: submitted.groupAction,
  createdAt: 1, cancellationRequested: false, counts: {}, objectiveVerified: false, children: [],
});

it('confirms the original accepted request after a lost ACK and a rejected revision without clearing the new draft', async () => {
  const owner = {}, dispatch = communeGroupDispatch(owner, 'host', 'group');
  dispatch.edit({ message: 'original' });
  const submit = vi.fn().mockRejectedValueOnce(new Error('lost reply'))
    .mockRejectedValueOnce(new Error('group idempotency conflict')).mockImplementation(async submitted => receipt(submitted));
  await dispatch.send(submit, request());
  dispatch.edit({ message: 'new draft' });
  const restored = communeGroupDispatch(owner, 'host', 'group');
  expect(restored).toBe(dispatch);
  await restored.send(submit, request('new draft'));
  expect(get(restored).pending?.request.message).toBe('original');
  await restored.send(submit);
  expect(submit.mock.calls[2][0]).toEqual(request());
  expect(submit.mock.calls.map(call => call[0].idempotency_key)).toEqual(['one-intent', 'one-intent', 'one-intent']);
  expect(get(restored).draft.message).toBe('new draft');
  expect(get(restored).pending).toBeUndefined();
  expect(get(restored).receipt?.groupTaskId).toBe('task');
});

it('lets an explicitly corrected unaccepted request use the same key, then starts a separate confirmed intent', async () => {
  const dispatch = communeGroupDispatch({}, 'host', 'group');
  const submit = vi.fn().mockRejectedValueOnce(new Error('invalid objective')).mockImplementation(async submitted => receipt(submitted));
  dispatch.edit({ message: 'original' });
  await dispatch.send(submit, request());
  dispatch.edit({ message: 'corrected' });
  await dispatch.send(submit, request('corrected'));
  expect(get(dispatch).draft.message).toBe('');
  expect(get(dispatch).pending).toBeUndefined();
  await dispatch.send(submit, { ...request('next'), idempotency_key: 'next-intent' });
  expect(submit.mock.calls[2][0].idempotency_key).toBe('next-intent');
});

it('sends once under concurrent clicks and retains typing before a late receipt', async () => {
  const dispatch = communeGroupDispatch({}, 'host', 'group');
  let acknowledge!: (task: CommuneGroupTask) => void;
  const submit = vi.fn(() => new Promise<CommuneGroupTask>(resolve => { acknowledge = resolve; }));
  dispatch.edit({ message: 'original' });
  const submitted = request();
  const first = dispatch.send(submit, submitted);
  submitted.targets[0].lease = 'mutated-after-send';
  await dispatch.send(submit, request());
  expect(submit).toHaveBeenCalledOnce();
  expect(get(dispatch).pending?.request.targets[0].lease).toBe('lease');
  dispatch.edit({ message: 'typed while pending' });
  acknowledge(receipt(request()));
  await first;
  expect(get(dispatch).draft.message).toBe('typed while pending');
});

it('confirms a corrected request when its acceptance reply was lost', async () => {
  const dispatch = communeGroupDispatch({}, 'host', 'group');
  const submit = vi.fn().mockRejectedValueOnce(new Error('invalid objective'))
    .mockRejectedValueOnce(new Error('lost corrected reply')).mockImplementation(async submitted => receipt(submitted));
  dispatch.edit({ message: 'original' });
  await dispatch.send(submit, request());
  dispatch.edit({ message: 'corrected' });
  await dispatch.send(submit, request('corrected'));
  expect(get(dispatch).pending?.request.message).toBe('corrected');
  await dispatch.send(submit);
  expect(submit.mock.calls[2][0]).toEqual(request('corrected'));
  expect(get(dispatch).pending).toBeUndefined();
  expect(get(dispatch).draft.message).toBe('');
});

it('isolates clients and scopes and refuses a new key while acceptance is unresolved', async () => {
  const owner = {}, dispatch = communeGroupDispatch(owner, 'host', 'group');
  await dispatch.send(vi.fn().mockRejectedValue(new Error('offline')), request());
  const submit = vi.fn();
  await dispatch.send(submit, { ...request(), idempotency_key: 'duplicate' });
  expect(submit).not.toHaveBeenCalled();
  expect(get(dispatch).pending?.request.idempotency_key).toBe('one-intent');
  expect(get(communeGroupDispatch({}, 'host', 'group')).pending).toBeUndefined();
  expect(get(communeGroupDispatch(owner, 'workspace', 'group')).pending).toBeUndefined();
  expect(get(communeGroupDispatch(owner, 'host', 'other')).pending).toBeUndefined();
});

it('retains confirmation ownership when an invalid or wrong-scope receipt arrives', async () => {
  const dispatch = communeGroupDispatch({}, 'host', 'group');
  const submit = vi.fn().mockResolvedValue({ ...receipt(request()), workspaceId: 'different-workspace' });
  await dispatch.send(submit, request());
  expect(get(dispatch).pending?.request).toEqual(request());
  expect(get(dispatch).receipt).toBeUndefined();
  expect(get(dispatch).error).toContain('different group dispatch receipt');
});
