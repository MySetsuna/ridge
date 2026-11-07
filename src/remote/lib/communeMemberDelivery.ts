import { writable } from 'svelte/store';
import type { AgentMessageReceipt, AgentMessageTarget, RemoteLink } from '@ridge/remote';

interface DeliveryState {
  draft: string;
  pending?: { message: string; key: string };
  receipt?: AgentMessageReceipt;
  busy?: 'send' | 'read' | 'cancel';
  error: string;
  cancellationRequested: boolean;
}

const deliveries = new WeakMap<RemoteLink, Map<string, MemberDelivery>>();
export const deliveryActive = (receipt?: AgentMessageReceipt): boolean =>
  ['submitted', 'queued', 'adapter_accepted', 'agent_received', 'agent_accepted', 'agent_acknowledged', 'dispatching', 'running', 'waiting_permission']
    .includes(receipt?.execution?.state ?? receipt?.status ?? '');

/** In-memory intent ownership survives UI remounts without writing private drafts to disk. */
export function memberDelivery(link: RemoteLink, target: AgentMessageTarget): MemberDelivery {
  let members = deliveries.get(link);
  if (!members) { members = new Map(); deliveries.set(link, members); }
  const key = JSON.stringify([target.workspaceId, target.paneId, target.agentId, target.generation, target.lease]);
  let delivery = members.get(key);
  if (!delivery) { delivery = createDelivery(link, { ...target }); members.set(key, delivery); }
  return delivery;
}

function createDelivery(link: RemoteLink, target: AgentMessageTarget) {
  let state: DeliveryState = { draft: '', error: '', cancellationRequested: false };
  const store = writable(state);
  const update = (patch: Partial<DeliveryState>) => { state = { ...state, ...patch }; store.set(state); };
  const exact = !!(target.workspaceId && target.paneId && target.agentId && target.generation && target.lease);
  const online = () => link.state() === 'connected';

  return {
    subscribe: store.subscribe,
    setDraft(draft: string) { update({ draft }); },
    async send(allowed: boolean): Promise<void> {
      if (state.busy || !allowed || !exact || !online() || !link.hasCapability('send_agent_message')) return;
      const pending = state.pending ?? { message: state.draft.trim(), key: crypto.randomUUID() };
      if (!pending.message) return;
      const submittedDraft = state.draft;
      update({ pending, busy: 'send', error: '', receipt: undefined, cancellationRequested: false });
      try {
        const receipt = await link.sendAgentMessage(target, pending.message, pending.key);
        if (!receipt?.deliveryId) throw new Error('Agent Hub returned an invalid receipt');
        update({ receipt, pending: undefined, cancellationRequested: false,
          draft: state.draft === submittedDraft && submittedDraft.trim() === pending.message ? '' : state.draft });
      } catch (error) { update({ error: String(error) }); }
      finally { update({ busy: undefined }); }
    },
    async refresh(): Promise<void> {
      if (state.busy || !state.receipt || !online() || !link.hasCapability('get_agent_message_delivery_status')) return;
      update({ busy: 'read' });
      try {
        const receipt = await link.getAgentMessageDelivery(target, state.receipt!.deliveryId);
        if (receipt?.deliveryId !== state.receipt!.deliveryId) throw new Error('Agent Hub returned a different receipt');
        update({ receipt, error: '' });
      } catch (error) { update({ error: String(error) }); }
      finally { update({ busy: undefined }); }
    },
    async cancel(allowed: boolean): Promise<void> {
      if (state.busy || !allowed || !state.receipt || !deliveryActive(state.receipt) || !online() || !link.hasCapability('cancel_agent_message')) return;
      update({ busy: 'cancel', error: '' });
      try {
        const receipt = await link.cancelAgentMessage(target, state.receipt!.deliveryId);
        if (receipt?.deliveryId !== state.receipt!.deliveryId) throw new Error('Agent Hub returned a different receipt');
        update({ receipt, cancellationRequested: true });
      } catch (error) { update({ error: String(error) }); }
      finally { update({ busy: undefined }); }
    },
  };
}

export type MemberDelivery = ReturnType<typeof createDelivery>;
