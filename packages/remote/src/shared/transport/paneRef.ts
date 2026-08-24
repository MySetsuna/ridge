/** Composite identity for a remote pane. Pane ids are only unique per workspace. */
export interface PaneRef {
  workspaceId: string;
  paneId: string;
}

/** Side whose refresh established the pane's canonical render grid. */
export type PaneRenderOwner = 'host' | 'remote';

export function paneRefKey(ref: PaneRef): string {
  return `${ref.workspaceId}:${ref.paneId}`;
}
