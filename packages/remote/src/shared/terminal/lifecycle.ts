import type { PaneRef } from '../transport/paneRef';

export interface TerminalLifecycle extends PaneRef {
  runtimeEpoch: string;
  terminalEpoch: string;
  ptyGeneration: number;
  status: 'Idle' | 'Exited';
  error: string | null;
}

/** Read a Runtime projection; an older process or activation cannot overwrite it. */
export function readTerminalLifecycle(payload: unknown, pane: PaneRef, previous: TerminalLifecycle | null, activation?: number): TerminalLifecycle | null {
  if (!payload || typeof payload !== 'object') return null;
  const value = payload as Record<string, unknown>;
  if (value.workspaceId !== pane.workspaceId || value.paneId !== pane.paneId
    || typeof value.runtimeEpoch !== 'string' || !value.runtimeEpoch
    || typeof value.terminalEpoch !== 'string' || !value.terminalEpoch
    || typeof value.ptyGeneration !== 'number' || !Number.isSafeInteger(value.ptyGeneration) || value.ptyGeneration < 0
    || (value.status !== 'Idle' && value.status !== 'Exited')
    || (value.activationId !== undefined && value.activationId !== activation)) return null;
  if (previous?.workspaceId === pane.workspaceId && previous.paneId === pane.paneId
    && previous.runtimeEpoch === value.runtimeEpoch && previous.ptyGeneration > value.ptyGeneration) return null;
  if (previous?.workspaceId === pane.workspaceId && previous.paneId === pane.paneId && previous.runtimeEpoch === value.runtimeEpoch
    && previous.ptyGeneration === value.ptyGeneration && previous.terminalEpoch !== value.terminalEpoch) return null;
  return { ...pane, runtimeEpoch: value.runtimeEpoch, terminalEpoch: value.terminalEpoch,
    ptyGeneration: value.ptyGeneration, status: value.status, error: typeof value.error === 'string' && value.error ? value.error : null };
}

export function terminalLifecycleMessage(state: TerminalLifecycle | null): string | null {
  if (state?.error) return `Shell could not restart: ${state.error}`;
  return state?.status === 'Exited' ? 'Terminal exited. Choose a shell to start again.' : null;
}
