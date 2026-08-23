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
  agentHitlPendingStore,
  agentTopologyStore,
  pruneAgentPaneHighlightWorkspaces,
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
          rosterChanged: true,
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

    const result = await refreshAgentPaneHighlight({ workspaceIds: ['ws-1'], invoke });

    expect(get(agentPaneAttentionStore)).toEqual({ 'ws-1:pane-a': 'waiting' });
    expect(get(agentPaneStatusStore)).toEqual({ 'ws-1:pane-a': 'waiting' });
    expect(get(agentTopologyStore)['ws-1']?.roster[0]?.paneId).toBe('pane-a');
    expect(get(agentHitlPendingStore)).toEqual(result.pending);
    expect(result.pending).toEqual([{ id: 'hitl-1', initiator: 'pane-a', reason: 'rm -rf' }]);
    expect(result.rosterChanged).toBe(true);
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

  it('keeps unrefreshed workspace status and transition history intact', () => {
    const wsA = {
      workspaceId: 'ws-a',
      profile: profile({ id: 'agent-a', paneId: 'pane-a', status: 'Working', outputSeq: 1 }),
    };
    const wsB = {
      workspaceId: 'ws-b',
      profile: profile({ id: 'agent-b', paneId: 'pane-b', status: 'Working', outputSeq: 4 }),
    };
    syncAgentPaneHighlight([wsA, wsB], () => false);

    syncAgentPaneHighlight(
      [{ ...wsA, profile: { ...wsA.profile, activity: 'idle', outputSeq: 2 } }],
      () => false,
      ['ws-a'],
    );

    expect(get(agentPaneStatusStore)).toEqual({
      'ws-a:pane-a': 'idle',
      'ws-b:pane-b': 'working',
    });
    expect(get(agentPaneAttentionStore)).toEqual({ 'ws-a:pane-a': 'idle' });

    syncAgentPaneHighlight(
      [{ ...wsB, profile: { ...wsB.profile, activity: 'idle', outputSeq: 5 } }],
      () => false,
      ['ws-b'],
    );
    expect(get(agentPaneAttentionStore)).toEqual({
      'ws-a:pane-a': 'idle',
      'ws-b:pane-b': 'idle',
    });
  });

  it('prunes every pane runtime record when a workspace closes', () => {
    const wsA = {
      workspaceId: 'ws-a',
      profile: profile({ id: 'agent-a', paneId: 'pane-a', status: 'Working', outputSeq: 1 }),
    };
    const wsB = {
      workspaceId: 'ws-b',
      profile: profile({ id: 'agent-b', paneId: 'pane-b', status: 'Working', outputSeq: 2 }),
    };
    syncAgentPaneHighlight([wsA, wsB], () => false);
    agentPaneAttentionStore.set({ 'ws-a:pane-a': 'idle', 'ws-b:pane-b': 'waiting' });
    agentPaneAttentionPollingStoppedStore.set({ 'ws-a:pane-a': 'working', 'ws-b:pane-b': 'working' });

    pruneAgentPaneHighlightWorkspaces(['ws-b']);

    expect(get(agentPaneStatusStore)).toEqual({ 'ws-b:pane-b': 'working' });
    expect(get(agentPaneAttentionStore)).toEqual({ 'ws-b:pane-b': 'waiting' });
    expect(get(agentPaneAttentionPollingStoppedStore)).toEqual({ 'ws-b:pane-b': 'working' });

    // Reusing an old workspace/pane id starts fresh instead of latching its prior transition.
    syncAgentPaneHighlight(
      [{ ...wsA, profile: { ...wsA.profile, activity: 'idle', outputSeq: 0 } }],
      () => false,
      ['ws-a'],
    );
    expect(get(agentPaneAttentionStore)).toEqual({ 'ws-b:pane-b': 'waiting' });
  });
});
