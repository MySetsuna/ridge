import { afterEach, describe, expect, it } from 'vitest';
import { get } from 'svelte/store';
import { communeUnreadWorkspaces, isAgentMessageEvent, markCommuneRead, markCommuneUnread } from './communeAttention';

afterEach(() => communeUnreadWorkspaces.set({}));

describe('Commune workspace attention', () => {
  it('recognizes agent text updates but ignores tools and user messages', () => {
    expect(isAgentMessageEvent({ workspaceId: 'a', kind: 'agent_message_chunk' })).toBe(true);
    expect(isAgentMessageEvent({ params: { update: { sessionUpdate: 'protocol_item', item: { type: 'agentMessage' } } } })).toBe(true);
    expect(isAgentMessageEvent({ params: { update: { sessionUpdate: 'tool_call' } } })).toBe(false);
    expect(isAgentMessageEvent({ params: { update: { sessionUpdate: 'user_message_chunk' } } })).toBe(false);
    expect(isAgentMessageEvent({ phase: 'replay', params: { update: { sessionUpdate: 'agent_message_chunk' } } })).toBe(false);
  });

  it('latches unread state per workspace until Commune is viewed', () => {
    markCommuneUnread('workspace-a');
    markCommuneUnread('workspace-b');
    markCommuneUnread('workspace-a');
    markCommuneRead('workspace-a');
    expect(get(communeUnreadWorkspaces)).toEqual({ 'workspace-b': true });
  });
});
