import type {
  PaneInfo,
  RemoteLink,
  RemotePanel,
  WorkspaceInfo,
  WsMessage,
} from '@ridge/remote';

const sessionIds = new WeakMap<object, number>();
let nextSessionId = 1;

export function remoteSessionId(link: RemoteLink): number {
  let id = sessionIds.get(link);
  if (id === undefined) {
    id = nextSessionId++;
    sessionIds.set(link, id);
  }
  return id;
}

export const remoteQueryKeys = {
  workspaces: (sessionId: number) => ['remote', sessionId, 'workspaces'] as const,
  panes: (sessionId: number, workspaceId: string) =>
    ['remote', sessionId, 'panes', workspaceId] as const,
  capabilities: (sessionId: number) => ['remote', sessionId, 'capabilities'] as const,
};

export type RemoteCapabilities = {
  panels: Readonly<Record<RemotePanel, boolean>>;
  manageWorkspaces: boolean;
  managePanes: boolean;
  theme: boolean;
};

export function dedupeRemoteItems<T extends { id: string }>(items: readonly T[]): T[] {
  const byId = new Map<string, T>();
  for (const item of items) byId.set(item.id, item);
  return [...byId.values()];
}

export function mergeRemoteItems<T extends { id: string }>(
  current: readonly T[] | undefined,
  incoming: readonly T[],
): T[] {
  const byId = new Map((current ?? []).map((item) => [item.id, item]));
  for (const item of incoming) {
    byId.set(item.id, { ...byId.get(item.id), ...item });
  }
  return [...byId.values()];
}

/** One list-panes request → its next canonical snapshot. Pushes remain separately merged. */
export function requestPaneSnapshot(
  link: RemoteLink,
  workspaceId: string,
  signal?: AbortSignal,
): Promise<PaneInfo[]> {
  return new Promise((resolve, reject) => {
    let stop = () => {};
    const abort = () => {
      stop();
      reject(signal?.reason ?? new DOMException('Aborted', 'AbortError'));
    };
    stop = link.onMessage((message: WsMessage) => {
      if (message.type !== 'panes') return;
      if (message.workspaceId && message.workspaceId !== workspaceId) return;
      stop();
      signal?.removeEventListener('abort', abort);
      resolve(dedupeRemoteItems(message.panes));
    });
    if (signal?.aborted) {
      abort();
      return;
    }
    signal?.addEventListener('abort', abort, { once: true });
    link.listPanes();
  });
}

export async function requestWorkspaceSnapshot(link: RemoteLink): Promise<WorkspaceInfo[]> {
  const result = await link.listWorkspaces();
  return dedupeRemoteItems(result.workspaces ?? []);
}
