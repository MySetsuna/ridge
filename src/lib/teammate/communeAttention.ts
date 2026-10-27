import { writable } from 'svelte/store';
import { isAgentReplyKind } from './agentCommuneModel';

export const communeUnreadWorkspaces = writable<Record<string, true>>({});

export function isAgentMessageEvent(payload: unknown): boolean {
  if (!payload || typeof payload !== 'object') return false;
  const record = payload as Record<string, unknown>;
  if (record.phase === 'replay') return false;
  if (isAgentReplyKind(record.kind)) return true;
  const params = record.params as { update?: { sessionUpdate?: unknown; item?: { type?: unknown } } } | undefined;
  const update = params?.update;
  return update?.sessionUpdate === 'agent_message_chunk'
    || (update?.sessionUpdate === 'protocol_item' && update.item?.type === 'agentMessage');
}

export function markCommuneUnread(workspaceId: string): void {
  if (workspaceId) communeUnreadWorkspaces.update(current => current[workspaceId] ? current : { ...current, [workspaceId]: true });
}

export function markCommuneRead(workspaceId: string): void {
  if (!workspaceId) return;
  communeUnreadWorkspaces.update(current => {
    if (!current[workspaceId]) return current;
    const next = { ...current };
    delete next[workspaceId];
    return next;
  });
}
