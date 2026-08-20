import { afterEach, describe, expect, it } from 'vitest';
import { get } from 'svelte/store';
import {
  agentPaneAttentionStore,
  agentPaneAttentionPollingStoppedStore,
  agentPaneStatusStore,
  clearAgentPaneAttention,
} from '$lib/stores/paneTree';
import {
  refreshAgentPaneHighlight,
  resetAgentPaneHighlightSync,
  syncAgentPaneHighlight,
} from './agentPaneHighlightSync';
import type { TeammateProfile } from './teammateModel';

function profile(partial: Partial<TeammateProfile> & Pick<TeammateProfile, 'id' | 'paneId' | 'status' | 'outputSeq'>): TeammateProfile {
  return {
    name: partial.name ?? partial.id,
    role: 'Worker',
    isAuto: true,
    recentOutput: '',
    activity: partial.status === 'Working' ? 'working' : 'idle',
    ...partial,
  };
}

afterEach(() => {
  resetAgentPaneHighlightSync();
  agentPaneAttentionStore.set({});
  agentPaneAttentionPollingStoppedStore.set({});
  agentPaneStatusStore.set({});
});

describe('agent pane highlight data plane', () => {
  it('writes intervention attention from roster without mounting AgentCenterPanel', async () => {
    const invoke = async (cmd: string) => {
      if (cmd === 'get_teammate_topology') {
        return {
          roster: [{
            id: 'agent-1',
            name: 'Claude',
            paneId: 'pane-a',
            status: 'Working',
            activity: 'working',
            outputSeq: 4,
          }],
        };
      }
      if (cmd === 'list_hitl_pending') {
        return [{ id: 'hitl-1', initiator: 'pane-a', reason: 'rm -rf' }];
      }
      throw new Error(`unexpected ${cmd}`);
    };

    await refreshAgentPaneHighlight({ workspaceIds: ['ws-1'], invoke });

    expect(get(agentPaneAttentionStore)).toEqual({ 'ws-1:pane-a': 'waiting' });
    expect(get(agentPaneStatusStore)).toEqual({ 'ws-1:pane-a': 'waiting' });
  });

  it('does not stroke a merely working or idle pane', () => {
    syncAgentPaneHighlight(
      [{
        workspaceId: 'ws-1',
        profile: profile({ id: 'agent-1', name: 'Claude', paneId: 'pane-a', status: 'Working', outputSeq: 2 }),
      }],
      () => false,
    );
    expect(get(agentPaneAttentionStore)).toEqual({});
    expect(get(agentPaneStatusStore)).toEqual({ 'ws-1:pane-a': 'working' });

    syncAgentPaneHighlight(
      [{
        workspaceId: 'ws-1',
        profile: profile({ id: 'agent-1', name: 'Claude', paneId: 'pane-a', status: 'Idle', outputSeq: 3 }),
      }],
      () => false,
    );
    expect(get(agentPaneAttentionStore)).toEqual({ 'ws-1:pane-a': 'idle' });

    syncAgentPaneHighlight(
      [{
        workspaceId: 'ws-1',
        profile: profile({ id: 'agent-1', name: 'Claude', paneId: 'pane-a', status: 'Idle', outputSeq: 3 }),
      }],
      () => false,
    );
    expect(get(agentPaneAttentionStore)).toEqual({ 'ws-1:pane-a': 'idle' });
  });

  it('stops acknowledged idle polling until the pane changes state', () => {
    const member = {
      workspaceId: 'ws-1',
      profile: profile({ id: 'agent-1', paneId: 'pane-a', status: 'Working', outputSeq: 2 }),
    };
    syncAgentPaneHighlight([member], () => false);
    syncAgentPaneHighlight([{ ...member, profile: { ...member.profile, activity: 'idle', outputSeq: 3 } }], () => false);

    clearAgentPaneAttention('ws-1', 'pane-a');
    expect(get(agentPaneAttentionStore)).toEqual({});
    expect(get(agentPaneAttentionPollingStoppedStore)).toEqual({ 'ws-1:pane-a': 'idle' });

    syncAgentPaneHighlight([{ ...member, profile: { ...member.profile, activity: 'idle', outputSeq: 3 } }], () => false);
    expect(get(agentPaneAttentionStore)).toEqual({});
    expect(get(agentPaneAttentionPollingStoppedStore)).toEqual({ 'ws-1:pane-a': 'idle' });

    syncAgentPaneHighlight([member], () => false);
    expect(get(agentPaneAttentionPollingStoppedStore)).toEqual({});

    syncAgentPaneHighlight([{ ...member, profile: { ...member.profile, activity: 'idle', outputSeq: 4 } }], () => false);
    expect(get(agentPaneAttentionStore)).toEqual({ 'ws-1:pane-a': 'idle' });
  });
});
