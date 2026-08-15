import { get } from 'svelte/store';
import {
  agentPaneAttentionStore,
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
} from './teammateModel';

export type HighlightMember = {
  workspaceId: string;
  profile: Pick<TeammateProfile, 'paneId' | 'status' | 'outputSeq' | 'id' | 'name'>;
};

const observedSignals = new Map<string, AgentPaneAttention | null>();
const observedStatuses = new Map<string, AgentPaneStatus | null>();

export function resetAgentPaneHighlightSync(): void {
  observedSignals.clear();
  observedStatuses.clear();
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
): boolean {
  let completionDetected = false;
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
    const signal = latchAgentAttention({
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
    const oldPaneId = key.slice(separator + 1);
    const current = get(agentPaneAttentionStore)[key];
    if (!current) setAgentPaneAttention(oldWorkspaceId, oldPaneId, 'idle');
    setAgentPaneStatus(oldWorkspaceId, oldPaneId, null);
  }
  observedSignals.clear();
  observedStatuses.clear();
  for (const [key, value] of next) observedSignals.set(key, value);
  for (const [key, value] of nextStatuses) observedStatuses.set(key, value);
  return completionDetected;
}

export async function refreshAgentPaneHighlight(input: {
  workspaceIds: readonly string[];
  invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
}): Promise<boolean> {
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
  let pending: { initiator: string }[] = [];
  try {
    pending = parseHitlPendingList(await input.invoke('list_hitl_pending'));
  } catch {
    pending = [];
  }
  const members = snapshots.flatMap(([workspaceId, snapshot]) =>
    snapshot.roster.map((profile) => ({ workspaceId, profile })),
  );
  return syncAgentPaneHighlight(members, (member) =>
    pending.some((item) => pendingMatches(item.initiator, member.profile)),
  );
}
