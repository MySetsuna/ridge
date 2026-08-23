import { get, writable } from 'svelte/store';
import {
  agentPaneAttentionStore,
  agentPaneAttentionPollingStoppedStore,
  agentPaneStatusStore,
  setAgentPaneAttention,
  setAgentPaneStatus,
  type AgentPaneAttention,
  type AgentPaneStatus,
} from '$lib/stores/paneTree';
import {
  latchAgentAttention,
  agentPaneStatus,
} from './agentCommuneModel';
import {
  EMPTY_TOPOLOGY,
  parseTopologySnapshot,
  type TeammateProfile,
  type TopologySnapshot,
} from './teammateModel';

export const AGENT_ACTIVITY_EXPIRY_MS = 12_500;
export const agentTopologyStore = writable<Record<string, TopologySnapshot>>({});
export const agentHitlPendingStore = writable<unknown>([]);

export type HighlightMember = {
  workspaceId: string;
  profile: TeammateProfile;
};

export type AgentPaneHighlightRefresh = {
  completionDetected: boolean;
  pending: unknown;
  rosterChanged: boolean;
};

const observedSignals = new Map<string, AgentPaneAttention | null>();
const observedStatuses = new Map<string, AgentPaneStatus | null>();

export function resetAgentPaneHighlightSync(): void {
  observedSignals.clear();
  observedStatuses.clear();
  agentTopologyStore.set({});
  agentHitlPendingStore.set([]);
}

function prunePaneRecord<T>(current: Record<string, T>, live: Set<string>): Record<string, T> {
  const stale = Object.keys(current).filter((key) => {
    const separator = key.indexOf(':');
    return separator > 0 && !live.has(key.slice(0, separator));
  });
  if (stale.length === 0) return current;
  const next = { ...current };
  for (const key of stale) delete next[key];
  return next;
}

export function pruneAgentPaneHighlightWorkspaces(workspaceIds: readonly string[]): void {
  const live = new Set(workspaceIds);
  for (const key of observedSignals.keys()) {
    const separator = key.indexOf(':');
    if (separator > 0 && !live.has(key.slice(0, separator))) observedSignals.delete(key);
  }
  for (const key of observedStatuses.keys()) {
    const separator = key.indexOf(':');
    if (separator > 0 && !live.has(key.slice(0, separator))) observedStatuses.delete(key);
  }
  agentTopologyStore.update((current) => {
    const stale = Object.keys(current).filter((workspaceId) => !live.has(workspaceId));
    if (stale.length === 0) return current;
    const next = { ...current };
    for (const workspaceId of stale) delete next[workspaceId];
    return next;
  });
  agentPaneStatusStore.update((current) => prunePaneRecord(current, live));
  agentPaneAttentionStore.update((current) => prunePaneRecord(current, live));
  agentPaneAttentionPollingStoppedStore.update((current) => prunePaneRecord(current, live));
}

function pendingMatches(
  initiator: string,
  profile: Pick<TeammateProfile, 'paneId' | 'id' | 'name'>,
): boolean {
  return initiator === profile.paneId || initiator === profile.name || initiator === profile.id;
}

export function parseHitlPendingList(raw: unknown): { initiator: string }[] {
  if (!Array.isArray(raw)) return [];
  return raw.flatMap((value) => {
    const rec = value as Record<string, unknown> | null;
    if (!rec || typeof rec.id !== 'string') return [];
    return [{ initiator: typeof rec.initiator === 'string' ? rec.initiator : '' }];
  });
}

/** Write attention/status stores from a live roster. Independent of Agent tab mount. */
export function syncAgentPaneHighlight(
  members: readonly HighlightMember[],
  isPending: (member: HighlightMember) => boolean,
  workspaceIds: readonly string[] = members.map((member) => member.workspaceId),
): boolean {
  let completionDetected = false;
  const scope = new Set(workspaceIds);
  const next = new Map<string, AgentPaneAttention | null>();
  const nextStatuses = new Map<string, AgentPaneStatus | null>();
  for (const member of members) {
    const profile = member.profile;
    if (!profile.paneId) continue;
    const key = `${member.workspaceId}:${profile.paneId}`;
    const pending = isPending(member);
    const paneStatus = agentPaneStatus(profile, pending);
    const storedStatus = get(agentPaneStatusStore)[key];
    const previousStatus = observedStatuses.get(key) ?? storedStatus;
    const existing = get(agentPaneAttentionStore)[key] ?? null;
    const previous = observedSignals.get(key);
    // A click/focus acknowledges this exact state. Do not turn repeated roster
    // polls into another border until the Agent actually changes state.
    const pollingStopped = get(agentPaneAttentionPollingStoppedStore)[key] === paneStatus;
    const signal = pollingStopped ? null : latchAgentAttention({
      previousStatus,
      currentStatus: paneStatus,
      pending,
      profileStatus: profile.status,
      outputSeq: profile.outputSeq,
      existingAttention: existing,
      seenBefore: observedStatuses.has(key) || storedStatus !== undefined,
    });
    if (signal !== null && signal !== previous && signal !== existing) {
      if (signal === 'idle' || signal === 'stopped') completionDetected = true;
      setAgentPaneAttention(member.workspaceId, profile.paneId, signal);
    }
    setAgentPaneStatus(member.workspaceId, profile.paneId, paneStatus);
    next.set(key, signal);
    nextStatuses.set(key, paneStatus);
  }
  for (const key of observedSignals.keys()) {
    if (next.has(key)) continue;
    const separator = key.indexOf(':');
    if (separator <= 0) continue;
    const oldWorkspaceId = key.slice(0, separator);
    if (!scope.has(oldWorkspaceId)) continue;
    const oldPaneId = key.slice(separator + 1);
    const current = get(agentPaneAttentionStore)[key];
    if (!current) setAgentPaneAttention(oldWorkspaceId, oldPaneId, 'idle');
    setAgentPaneStatus(oldWorkspaceId, oldPaneId, null);
  }
  for (const key of observedSignals.keys()) {
    const separator = key.indexOf(':');
    if (separator > 0 && scope.has(key.slice(0, separator))) observedSignals.delete(key);
  }
  for (const key of observedStatuses.keys()) {
    const separator = key.indexOf(':');
    if (separator > 0 && scope.has(key.slice(0, separator))) observedStatuses.delete(key);
  }
  for (const [key, value] of next) observedSignals.set(key, value);
  for (const [key, value] of nextStatuses) observedStatuses.set(key, value);
  return completionDetected;
}

export async function refreshAgentPaneHighlight(input: {
  workspaceIds: readonly string[];
  invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
}): Promise<AgentPaneHighlightRefresh> {
  const snapshots = await Promise.all(
    input.workspaceIds.map(async (workspaceId) => {
      try {
        return [
          workspaceId,
          parseTopologySnapshot(await input.invoke('get_teammate_topology', { workspaceId })),
        ] as const;
      } catch {
        return [workspaceId, EMPTY_TOPOLOGY] as const;
      }
    }),
  );
  let pendingRaw: unknown = [];
  try {
    pendingRaw = await input.invoke('list_hitl_pending');
  } catch {
    pendingRaw = [];
  }
  const pending = parseHitlPendingList(pendingRaw);
  agentHitlPendingStore.set(pendingRaw);
  const members = snapshots.flatMap(([workspaceId, snapshot]) =>
    snapshot.roster.map((profile) => ({ workspaceId, profile })),
  );
  agentTopologyStore.update((current) => {
    const next = { ...current };
    for (const [workspaceId, snapshot] of snapshots) next[workspaceId] = snapshot;
    return next;
  });
  return {
    completionDetected: syncAgentPaneHighlight(
      members,
      (member) => pending.some((item) => pendingMatches(item.initiator, member.profile)),
      input.workspaceIds,
    ),
    pending: pendingRaw,
    rosterChanged: snapshots.some(([, snapshot]) => snapshot.rosterChanged),
  };
}
