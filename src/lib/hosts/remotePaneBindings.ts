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
  activationId: number;
  recoveryAttempts: number;
  unlisten: () => void;
}

const bindings = new Map<string, LiveBinding>();
let activationCounter = 0;

export function bindRemotePane(binding: RemotePaneBinding): void {
  if (!binding.link.onTerminalFrame) throw new Error('canonical terminal transport unavailable');
  dropBinding(binding.localPaneId, false);
  const live: LiveBinding = {
    ...binding,
    active: false,
    activationId: ++activationCounter,
    recoveryAttempts: 0,
    unlisten: () => {},
  };
  const pane: PaneRef = {
    workspaceId: binding.workspaceId,
    paneId: binding.remotePaneId,
  };
  const stops: Array<() => void> = [];
  stops.push(binding.link.onTerminalFrame(bytes => {
    if (!live.active) return;
    try {
      const manager = TerminalManager.instance();
      manager.applyRemoteFrame(binding.localPaneId, bytes, pane.workspaceId, pane.paneId, live.activationId);
      const context = manager.getKernel(binding.localPaneId)?.terminalInputContext();
      if (context?.[2] === live.activationId) live.recoveryAttempts = 0;
    } catch (error) {
      console.warn('[remote-pane] terminal snapshot required', error);
      if (++live.recoveryAttempts > 3) return;
      live.activationId = ++activationCounter;
      binding.link.subscribePane(pane, { activationId: live.activationId, active: true });
    }
  }));
  if (binding.link.onPtyResize) {
    stops.push(binding.link.onPtyResize((incoming, rows, cols) => {
      if (incoming.paneId !== pane.paneId || incoming.workspaceId !== pane.workspaceId) return;
      TerminalManager.instance().applyPaneResize(binding.localPaneId, rows, cols);
    }));
  }
  live.unlisten = () => { for (const stop of stops) stop(); };
  bindings.set(binding.localPaneId, live);
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
  if (!live.active) {
    live.active = true;
    live.link.subscribePane({ workspaceId: live.workspaceId, paneId: live.remotePaneId }, { activationId: live.activationId, active: true });
  }
  promoteRemotePaneBinding(localPaneId);
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
