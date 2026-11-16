import { writable } from 'svelte/store';
import { RpcRemoteError } from '../../../packages/remote/src/shared/transport/types';

export interface LaunchOptions {
  runtimeEpoch: string;
  cwd: string | null;
  profiles: { id: string; name?: string; collaboration: { protocol: string; command: string; args: string[] } }[];
}
export interface LaunchRequest {
  workspaceId: string;
  paneId: string;
  runtimeEpoch: string;
  entry: 'agent';
  cwd: string;
  collaboration: { profile: string; arguments: string[] } | { connect: string; protocol: SharedProtocol; bearerTokenFile?: string; arguments: string[] };
}
export type SharedProtocol = 'acp' | 'acp-ws' | 'app-server';
type LaunchSelection = { cwd: string; runtimeEpoch: string } & ({ profile: string } | { connect: string; protocol: SharedProtocol; bearerTokenFile?: string });
export interface LaunchReceipt { workspaceId: string; paneId: string }

export function createCommuneLaunch() {
  let state: { pending?: LaunchRequest; busy: boolean; editable: boolean; error: string } = {
    busy: false, editable: true, error: '',
  };
  const store = writable(state);
  const update = (patch: Partial<typeof state>) => { state = { ...state, ...patch }; store.set(state); };
  return {
    subscribe: store.subscribe,
    async send(submit: (request: LaunchRequest) => Promise<LaunchReceipt>, selection: LaunchSelection) {
      if (state.busy) return;
      const previous = state.pending;
      const request: LaunchRequest = previous && !state.editable ? previous : {
        workspaceId: previous?.workspaceId ?? crypto.randomUUID(),
        paneId: previous?.paneId ?? crypto.randomUUID(),
        runtimeEpoch: previous?.runtimeEpoch ?? selection.runtimeEpoch,
        entry: 'agent', cwd: selection.cwd,
        collaboration: 'connect' in selection
          ? { connect: selection.connect, protocol: selection.protocol, ...(selection.bearerTokenFile ? { bearerTokenFile: selection.bearerTokenFile } : {}), arguments: [] }
          : { profile: selection.profile, arguments: [] },
      };
      update({ pending: structuredClone(request), busy: true, editable: false, error: '' });
      try {
        const receipt = await submit(structuredClone(request));
        if (receipt?.workspaceId !== request.workspaceId || receipt.paneId !== request.paneId) {
          throw new Error('Runtime returned a different collaboration terminal identity');
        }
        update({ pending: undefined, editable: true });
        return receipt;
      } catch (error) {
        // A confirmed rejection permits correction under the same identity. Unknown results stay frozen.
        update({ error: String(error), editable: error instanceof RpcRemoteError });
      } finally { update({ busy: false }); }
    },
  };
}

/** Preserve an unconfirmed native launch through dialog unmounts. */
export const nativeCommuneLaunch = createCommuneLaunch();
