import type { RemotePanel } from '@ridge/remote';

export type RemoteViewer = {
  kind: 'file' | 'diff';
  path: string;
  line?: number;
};

/** Pure browser/UI state. Remote snapshots and terminal kernels live elsewhere. */
export class MobileRemoteUiState {
  activePaneId = $state<string | null>(null);
  activeWorkspaceId = $state('');
  selectionMode = $state(false);
  sentenceBuffer = $state(false);
  sidebarTab = $state<RemotePanel | null>(null);
  viewer = $state<RemoteViewer | null>(null);
  showKeyboard = $state(true);
  keyboardShift = $state(0);

  constructor(sentenceBuffer: boolean) {
    this.sentenceBuffer = sentenceBuffer;
  }
}
