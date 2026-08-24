import { TerminalManager } from '@ridge/remote/shared/terminal/manager';
import type { PaneRef } from '@ridge/remote';
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
  const pane: PaneRef = {
    workspaceId: binding.workspaceId,
    paneId: binding.remotePaneId,
  };
  const stops: Array<() => void> = [];
  stops.push(binding.link.onRawBytes((incoming, bytes) => {
    if (incoming.paneId !== pane.paneId || incoming.workspaceId !== pane.workspaceId) return;
    if (live.active) {
      const manager = TerminalManager.instance();
      manager.enqueueFeed?.(binding.localPaneId, bytes);
      return;
    }
    live.pending.push(bytes.slice());
    live.pendingBytes += bytes.byteLength;
    while (live.pendingBytes > MAX_PENDING_BYTES && live.pending.length > 1) {
      live.pendingBytes -= live.pending.shift()!.byteLength;
    }
  }));
  if (binding.link.onPtyResize) {
    stops.push(binding.link.onPtyResize((incoming, rows, cols) => {
      if (incoming.paneId !== pane.paneId || incoming.workspaceId !== pane.workspaceId) return;
      TerminalManager.instance().applyPaneResize(binding.localPaneId, rows, cols);
    }));
  }
  live.unlisten = () => { for (const stop of stops) stop(); };
  for (const text of binding.link.getPaneOutput(pane)) {
    const bytes = new TextEncoder().encode(text);
    live.pending.push(bytes);
    live.pendingBytes += bytes.byteLength;
  }
  bindings.set(binding.localPaneId, live);
  binding.link.subscribePane(pane);
}

export function remotePaneBinding(localPaneId: string): RemotePaneBinding | undefined {
  return bindings.get(localPaneId);
}

export function terminalPathOrigin(localPaneId: string): {
  kind: 'remote' | 'rdg';
  hostId: string;
  inspectPath?: HostTopologyLink['inspectPath'];
} | null {
  const binding = bindings.get(localPaneId);
  if (!binding) return null;
  return {
    kind: binding.hostId.startsWith('rdg:') ? 'rdg' : 'remote',
    hostId: binding.hostId,
    inspectPath: binding.link.inspectPath?.bind(binding.link),
  };
}

export function localPaneIdsForRemote(hostId: string, remotePaneId: string): string[] {
  return [...bindings.values()]
    .filter((binding) =>
      binding.hostId === hostId && binding.remotePaneId === remotePaneId)
    .map((binding) => binding.localPaneId);
}

export async function deleteRemotePane(
  hostId: string,
  workspaceIdOrRemotePaneId: string,
  remotePaneIdOrLink: string | HostTopologyLink,
  linkOrCloseLocal: HostTopologyLink | ((localPaneId: string) => Promise<void>),
  maybeCloseLocal?: (localPaneId: string) => Promise<void>,
): Promise<boolean> {
  const legacy = typeof remotePaneIdOrLink !== 'string';
  const workspaceId = legacy ? '' : workspaceIdOrRemotePaneId;
  const remotePaneId = legacy ? workspaceIdOrRemotePaneId : remotePaneIdOrLink;
  const link = (legacy ? remotePaneIdOrLink : linkOrCloseLocal) as HostTopologyLink;
  const closeLocalPane = (legacy ? linkOrCloseLocal : maybeCloseLocal) as (localPaneId: string) => Promise<void>;
  if (typeof link.closePane !== 'function') return false;
  if (!await link.closePane({ workspaceId, paneId: remotePaneId })) return false;
  for (const localPaneId of localPaneIdsForRemote(hostId, remotePaneId)) {
    await closeLocalPane(localPaneId);
  }
  return true;
}

export function activateRemotePaneBinding(localPaneId: string): void {
  const live = bindings.get(localPaneId);
  if (!live) return;
  live.active = true;
  promoteRemotePaneBinding(localPaneId);
  const manager = TerminalManager.instance();
  for (const bytes of live.pending) manager.enqueueFeed?.(localPaneId, bytes);
  live.pending = [];
  live.pendingBytes = 0;
}

/** Promote a mounted foreign pane when the user takes focus. */
export function promoteRemotePaneBinding(localPaneId: string): void {
  const live = bindings.get(localPaneId);
  if (!live) return;
  live.link.promotePane?.({
    workspaceId: live.workspaceId,
    paneId: live.remotePaneId,
  });
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
