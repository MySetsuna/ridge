import type { RemotePanel } from '@ridge/remote';
import type { PaneRef } from '@ridge/remote';

export type RemoteViewer = {
  kind: 'file' | 'diff';
  path: string;
  line?: number;
};

/** Pure browser/UI state. Remote snapshots and terminal kernels live elsewhere. */
export class MobileRemoteUiState {
  /** Workspace + pane move as one value so effects never observe a cross-wired pair. */
  private activeLocation = $state<{ workspaceId: string; paneId: string | null }>({
    workspaceId: '',
    paneId: null,
  });
  selectionMode = $state(false);
  sentenceBuffer = $state(false);
  sidebarTab = $state<RemotePanel | null>(null);
  viewer = $state<RemoteViewer | null>(null);
  showKeyboard = $state(true);
  keyboardShift = $state(0);

  constructor(sentenceBuffer: boolean) {
    this.sentenceBuffer = sentenceBuffer;
  }

  get activeWorkspaceId(): string {
    return this.activeLocation.workspaceId;
  }

  set activeWorkspaceId(workspaceId: string) {
    this.activeLocation = {
      workspaceId,
      paneId: workspaceId === this.activeLocation.workspaceId
        ? this.activeLocation.paneId
        : null,
    };
  }

  get activePaneId(): string | null {
    return this.activeLocation.paneId;
  }

  set activePaneId(paneId: string | null) {
    this.activeLocation = { ...this.activeLocation, paneId };
  }

  activePaneRef(): PaneRef | null {
    const { workspaceId, paneId } = this.activeLocation;
    return workspaceId && paneId ? { workspaceId, paneId } : null;
  }

  navigate(workspaceId: string, paneId: string | null): void {
    this.activeLocation = { workspaceId, paneId };
  }
}
