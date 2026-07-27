import { TerminalManager } from '@ridge/remote/shared/terminal/manager';
import type { HostTopologyLink } from './hostForest';

export interface RemotePaneBinding {
  localPaneId: string;
  hostId: string;
  workspaceId: string;
  remotePaneId: string;
  link: HostTopologyLink;
}

interface LiveBinding extends RemotePaneBinding {
  active: boolean;
  pending: Uint8Array[];
  pendingBytes: number;
  unlisten: () => void;
}

const MAX_PENDING_BYTES = 1024 * 1024;
const bindings = new Map<string, LiveBinding>();

export function bindRemotePane(binding: RemotePaneBinding): void {
  dropBinding(binding.localPaneId, false);
  const live: LiveBinding = {
    ...binding,
    active: false,
    pending: [],
    pendingBytes: 0,
    unlisten: () => {},
  };
  live.unlisten = binding.link.onRawBytes((paneId, bytes) => {
    if (paneId !== binding.remotePaneId) return;
    if (live.active) {
      TerminalManager.instance().feed(binding.localPaneId, bytes);
      return;
    }
    live.pending.push(bytes.slice());
    live.pendingBytes += bytes.byteLength;
    while (live.pendingBytes > MAX_PENDING_BYTES && live.pending.length > 1) {
      live.pendingBytes -= live.pending.shift()!.byteLength;
    }
  });
  for (const text of binding.link.getPaneOutput(binding.remotePaneId)) {
    const bytes = new TextEncoder().encode(text);
    live.pending.push(bytes);
    live.pendingBytes += bytes.byteLength;
  }
  bindings.set(binding.localPaneId, live);
  binding.link.subscribePane(binding.remotePaneId);
}

export function remotePaneBinding(localPaneId: string): RemotePaneBinding | undefined {
  return bindings.get(localPaneId);
}

export function localPaneIdsForRemote(hostId: string, remotePaneId: string): string[] {
  return [...bindings.values()]
    .filter((binding) =>
      binding.hostId === hostId && binding.remotePaneId === remotePaneId)
    .map((binding) => binding.localPaneId);
}

export async function deleteRemotePane(
  hostId: string,
  remotePaneId: string,
  link: HostTopologyLink,
  closeLocalPane: (localPaneId: string) => Promise<void>,
): Promise<boolean> {
  if (!await link.closePane(remotePaneId)) return false;
  for (const localPaneId of localPaneIdsForRemote(hostId, remotePaneId)) {
    await closeLocalPane(localPaneId);
  }
  return true;
}

export function activateRemotePaneBinding(localPaneId: string): void {
  const live = bindings.get(localPaneId);
  if (!live) return;
  live.active = true;
  const manager = TerminalManager.instance();
  for (const bytes of live.pending) manager.feed(localPaneId, bytes);
  live.pending = [];
  live.pendingBytes = 0;
}

export function unbindRemotePane(localPaneId: string): void {
  dropBinding(localPaneId, true);
}

function dropBinding(localPaneId: string, disconnectIfLast: boolean): void {
  const live = bindings.get(localPaneId);
  if (!live) return;
  live.unlisten();
  bindings.delete(localPaneId);
  if (
    disconnectIfLast
    && ![...bindings.values()].some((binding) => binding.hostId === live.hostId)
  ) {
    live.link.disconnect();
  }
}
