/**
 * Foreign pane UI status machine (OP-WS-LIFE / OP-BP-GUARD UI slice).
 * Pure — Hosts panel / RidgePane badge consume these.
 */

export type ForeignHostLink = 'connected' | 'connecting' | 'disconnected' | 'error';

export type ForeignPaneBadge =
  | { kind: 'live'; label: string }
  | { kind: 'reconnecting'; label: string; attempt: number }
  | { kind: 'detached'; label: string }
  | { kind: 'error'; label: string; detail?: string };

export interface ForeignPaneStatusInput {
  hostStatus: ForeignHostLink;
  subscribed: boolean;
  attachedLocally: boolean;
  reconnectAttempt: number;
  lastError?: string | null;
  hostLabel: string;
}

export function decideForeignPaneBadge(input: ForeignPaneStatusInput): ForeignPaneBadge {
  if (!input.attachedLocally) {
    return { kind: 'detached', label: '视图已断开 · 远端继续' };
  }
  if (input.hostStatus === 'error') {
    return {
      kind: 'error',
      label: `${input.hostLabel} 错误`,
      detail: input.lastError ?? undefined,
    };
  }
  if (input.hostStatus === 'disconnected' || input.hostStatus === 'connecting') {
    const attempt = Math.max(0, input.reconnectAttempt);
    return {
      kind: 'reconnecting',
      label: attempt > 0 ? `重连中 (#${attempt})` : '主机断开 · 重连中',
      attempt,
    };
  }
  if (!input.subscribed) {
    return {
      kind: 'reconnecting',
      label: '订阅恢复中',
      attempt: Math.max(0, input.reconnectAttempt),
    };
  }
  return { kind: 'live', label: `远端 · ${input.hostLabel}` };
}

/** Close-leaf copy: detach only, never imply kill remote PTY. */
export function foreignCloseConfirmMessage(hostLabel: string): string {
  return `断开与「${hostLabel}」的本地视图？远端会话将继续运行。`;
}
