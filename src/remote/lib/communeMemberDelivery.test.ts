import { get } from 'svelte/store';
import { describe, expect, it, vi } from 'vitest';
import type { AgentMessageReceipt, RemoteLink } from '@ridge/remote';
import { deliveryActive, memberDelivery } from './communeMemberDelivery';

const target = { workspaceId: 'workspace', paneId: 'pane', agentId: 'member', generation: 1, lease: 'lease-1' };
const receipt = { deliveryId: 'delivery-1', status: 'queued', execution: { state: 'running' } } as AgentMessageReceipt;
function fixture() {
  const link = {
    state: vi.fn(() => 'connected'), hasCapability: vi.fn(() => true),
    sendAgentMessage: vi.fn<RemoteLink['sendAgentMessage']>(async () => receipt),
    getAgentMessageDelivery: vi.fn<RemoteLink['getAgentMessageDelivery']>(async () => receipt),
    cancelAgentMessage: vi.fn<RemoteLink['cancelAgentMessage']>(async () => receipt),
  };
  return { link, delivery: memberDelivery(link as unknown as RemoteLink, target) };
}

describe('Remote single-member delivery intent', () => {
  it('continues reading an adapter receipt until actual protocol execution settles', () => {
    expect(deliveryActive({ ...receipt, status: 'adapter_accepted', execution: undefined })).toBe(true);
    expect(deliveryActive({ ...receipt, execution: { state: 'turn_ended' } })).toBe(false);
    expect(deliveryActive({ ...receipt, execution: { state: 'unknown' } })).toBe(false);
  });
  it('retains the original key, text and exact target after a committed request loses its ACK', async () => {
    const { link, delivery } = fixture();
    link.sendAgentMessage.mockRejectedValueOnce(new Error('lost ACK'));
    delivery.setDraft('original');
    await delivery.send(true);
    const original = link.sendAgentMessage.mock.calls[0];
    expect(get(delivery).pending?.key).toBe(original[2]);
    delivery.setDraft('new draft');
    await delivery.send(true);
    expect(link.sendAgentMessage.mock.calls[1]).toEqual(original);
    expect(original.slice(0, 2)).toEqual([target, 'original']);
    expect(get(delivery)).toMatchObject({ draft: 'new draft', receipt, pending: undefined, error: '' });
  });

  it('blocks simultaneous submissions and preserves typing while an ACK is delayed', async () => {
    const { link, delivery } = fixture();
    let acknowledge!: (value: AgentMessageReceipt) => void;
    link.sendAgentMessage.mockImplementationOnce(() => new Promise(resolve => { acknowledge = resolve; }));
    delivery.setDraft('first');
    const sending = delivery.send(true);
    await delivery.send(true);
    delivery.setDraft('second');
    acknowledge(receipt);
    await sending;
    expect(link.sendAgentMessage).toHaveBeenCalledTimes(1);
    expect(get(delivery).draft).toBe('second');
    await delivery.send(true);
    expect(link.sendAgentMessage.mock.calls[1][2]).not.toBe(link.sendAgentMessage.mock.calls[0][2]);
  });

  it('clears a confirmed unchanged draft and gives an intentional same-text send a new key', async () => {
    const { link, delivery } = fixture();
    delivery.setDraft('same text');
    await delivery.send(true);
    expect(get(delivery).draft).toBe('');
    delivery.setDraft('same text');
    await delivery.send(true);
    expect(link.sendAgentMessage.mock.calls[0][2]).not.toBe(link.sendAgentMessage.mock.calls[1][2]);
  });

  it('preserves an unresolved intent across remounts and separates connections, workspaces and generations', async () => {
    const { link, delivery } = fixture();
    link.sendAgentMessage.mockRejectedValueOnce(new Error('lost ACK'));
    delivery.setDraft('original');
    await delivery.send(true);
    expect(memberDelivery(link as unknown as RemoteLink, { ...target })).toBe(delivery);
    const otherWorkspace = memberDelivery(link as unknown as RemoteLink, { ...target, workspaceId: 'other' });
    const newGeneration = memberDelivery(link as unknown as RemoteLink, { ...target, generation: 2, lease: 'lease-2' });
    expect(get(otherWorkspace).pending).toBeUndefined();
    expect(get(newGeneration).pending).toBeUndefined();
    expect(fixture().delivery).not.toBe(delivery);
    expect(link.sendAgentMessage).toHaveBeenCalledTimes(1);
  });

  it('does not submit without current permissions, connection or exact binding', async () => {
    const { link, delivery } = fixture();
    delivery.setDraft('hello');
    await delivery.send(false);
    link.state.mockReturnValue('disconnected');
    await delivery.send(true);
    link.state.mockReturnValue('connected');
    link.hasCapability.mockReturnValue(false);
    await delivery.send(true);
    link.hasCapability.mockReturnValue(true);
    const unbound = memberDelivery(link as unknown as RemoteLink, { ...target, lease: undefined });
    unbound.setDraft('hello');
    await unbound.send(true);
    expect(link.sendAgentMessage).not.toHaveBeenCalled();
  });

  it('retains a rejected intent through permission revocation and retries only after authorization returns', async () => {
    const { link, delivery } = fixture();
    link.sendAgentMessage.mockRejectedValueOnce(new Error('permission denied'));
    delivery.setDraft('hello');
    await delivery.send(true);
    expect(get(delivery)).toMatchObject({ draft: 'hello', error: 'Error: permission denied' });
    link.hasCapability.mockReturnValue(false);
    await delivery.send(true);
    expect(link.sendAgentMessage).toHaveBeenCalledTimes(1);
    link.hasCapability.mockReturnValue(true);
    await delivery.send(true);
    expect(link.sendAgentMessage.mock.calls[0]).toEqual(link.sendAgentMessage.mock.calls[1]);
  });

  it('keeps the last receipt on failed reads and uses original identity for status and cancellation', async () => {
    const { link, delivery } = fixture();
    delivery.setDraft('hello');
    await delivery.send(true);
    link.getAgentMessageDelivery.mockRejectedValueOnce(new Error('offline'));
    await delivery.refresh();
    expect(get(delivery)).toMatchObject({ receipt, error: 'Error: offline' });
    link.cancelAgentMessage.mockRejectedValueOnce(new Error('cancel ACK lost'));
    await delivery.cancel(true);
    expect(get(delivery)).toMatchObject({ receipt, error: 'Error: cancel ACK lost', cancellationRequested: false });
    await delivery.refresh();
    expect(link.getAgentMessageDelivery).toHaveBeenLastCalledWith(target, receipt.deliveryId);
    await delivery.cancel(true);
    expect(link.cancelAgentMessage).toHaveBeenLastCalledWith(target, receipt.deliveryId);
    expect(get(delivery)).toMatchObject({ receipt, cancellationRequested: true });
    link.getAgentMessageDelivery.mockResolvedValueOnce({ ...receipt, execution: { state: 'cancelled' } });
    await delivery.refresh();
    await delivery.cancel(true);
    expect(link.cancelAgentMessage).toHaveBeenCalledTimes(2);
  });

  it('retains pending intent on an invalid ACK and rejects status for another delivery', async () => {
    const { link, delivery } = fixture();
    link.sendAgentMessage.mockResolvedValueOnce({} as AgentMessageReceipt);
    delivery.setDraft('hello');
    await delivery.send(true);
    expect(get(delivery).pending?.message).toBe('hello');
    await delivery.send(true);
    link.getAgentMessageDelivery.mockResolvedValueOnce({ ...receipt, deliveryId: 'wrong' });
    await delivery.refresh();
    expect(get(delivery).receipt).toEqual(receipt);
    expect(get(delivery).error).toContain('different receipt');
  });
});
