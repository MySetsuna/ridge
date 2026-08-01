/** Composite identity for a remote pane. Pane ids are only unique per workspace. */
export interface PaneRef {
  workspaceId: string;
  paneId: string;
}

export function paneRefKey(ref: PaneRef): string {
  return `${ref.workspaceId}:${ref.paneId}`;
}
