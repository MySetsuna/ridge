import { writable } from 'svelte/store';
import type { CommuneGroupDispatchRequest, CommuneGroupTask } from '../../../packages/remote/src/shared/transport/wsRemote';

interface Draft {
  mode: 'broadcast' | 'assign';
  selected: Record<string, { generation: number; lease: string }>;
  selectedRevision?: number;
  message: string;
  objectives: Record<string, string>;
}
interface DispatchState {
  draft: Draft;
  pending?: PendingDispatch;
  receipt?: CommuneGroupTask;
  busy: boolean;
  error: string;
}
interface PendingDispatch {
  request: CommuneGroupDispatchRequest;
  draft: Draft;
  previous?: PendingDispatch;
}
const emptyDraft = (): Draft => ({ mode: 'broadcast', selected: {}, message: '', objectives: {} });
export const nativeGroupDispatchOwner = {};
const clients = new WeakMap<object, Map<string, GroupDispatch>>();

/** Keep private drafts and unconfirmed request ownership across UI remounts, in memory only. */
export function communeGroupDispatch(owner: object, scope: string, groupId: string): GroupDispatch {
  let groups = clients.get(owner);
  if (!groups) { groups = new Map(); clients.set(owner, groups); }
  const key = JSON.stringify([scope, groupId]);
  let dispatch = groups.get(key);
  if (!dispatch) { dispatch = createDispatch(scope, groupId); groups.set(key, dispatch); }
  return dispatch;
}

function createDispatch(scope: string, groupId: string) {
  let state: DispatchState = { draft: emptyDraft(), busy: false, error: '' };
  const store = writable(state);
  const update = (patch: Partial<DispatchState>) => { state = { ...state, ...patch }; store.set(state); };
  return {
    subscribe: store.subscribe,
    edit(patch: Partial<Draft>) { update({ draft: { ...state.draft, ...patch } }); },
    async send(submit: (request: CommuneGroupDispatchRequest) => Promise<CommuneGroupTask>, request?: CommuneGroupDispatchRequest) {
      if (state.busy) return;
      const submission = request ?? state.pending?.request;
      if (!submission) return;
      const targetScope = submission.groupScope === 'host' ? 'host' : submission.workspaceId;
      if (submission.groupId !== groupId || targetScope !== scope
        || (state.pending && submission.idempotency_key !== state.pending.request.idempotency_key)) {
        update({ error: 'Group dispatch identity changed before confirmation' });
        return;
      }
      let pending = state.pending;
      if (!pending || (request && JSON.stringify(request) !== JSON.stringify(pending.request))) {
        pending = { request: structuredClone(submission), draft: state.draft, previous: pending };
      }
      // An explicit revised submission can only replace an unaccepted request under the same key.
      // Runtime rejects changes to an accepted request. Original confirmation retains its snapshot.
      const submittedDraft = request ? state.draft : pending.draft;
      const exactRequest = structuredClone(submission);
      update({ pending, busy: true, error: '' });
      try {
        const receipt = await submit(exactRequest);
        if (!receipt?.groupTaskId || receipt.groupId !== exactRequest.groupId
          || receipt.groupRevision !== exactRequest.groupRevision || receipt.action !== exactRequest.groupAction
          || (receipt.groupScope ?? undefined) !== (exactRequest.groupScope ?? undefined)
          || (receipt.workspaceId ?? undefined) !== (exactRequest.workspaceId ?? undefined)) {
          throw new Error('Runtime returned a different group dispatch receipt');
        }
        update({ receipt, pending: undefined, draft: state.draft === submittedDraft ? { ...emptyDraft(), mode: state.draft.mode } : state.draft });
        return receipt;
      } catch (failure) {
        // A conflict proves this candidate was not accepted. An unknown failure keeps it.
        const error = String(failure);
        update({ error, pending: error.includes('group idempotency conflict') && pending.previous ? pending.previous : pending });
      }
      finally { update({ busy: false }); }
    },
  };
}

export type GroupDispatch = ReturnType<typeof createDispatch>;
