import type { TeammateProfile } from './teammateModel';

export type AgentCardStatus = 'working' | 'waiting' | 'idle' | 'completed' | 'stopped';

export interface AgentHistoryReplyLike {
  agent: string;
  sessionId: string;
  timestamp: number;
}

export interface AgentHistoryGroup<T extends AgentHistoryReplyLike = AgentHistoryReplyLike> {
  key: string;
  agent: string;
  replies: T[];
}

/** Stable identity: history grouping must not depend on cwd or display casing. */
export function normalizeAgentIdentity(agent: string): string {
  const normalized = agent.trim().toLocaleLowerCase();
  return normalized || 'unknown';
}

/** Group all sessions for an agent, regardless of their recorded cwd. */
export function buildAgentHistoryGroups<T extends AgentHistoryReplyLike>(
  replies: readonly T[],
): AgentHistoryGroup<T>[] {
  const groups = new Map<string, AgentHistoryGroup<T>>();
  for (const reply of replies) {
    const key = normalizeAgentIdentity(reply.agent);
    const current = groups.get(key);
    if (current) {
      current.replies.push(reply);
    } else {
      groups.set(key, { key, agent: reply.agent.trim() || 'Unknown', replies: [reply] });
    }
  }
  for (const group of groups.values()) {
    group.replies.sort((a, b) => b.timestamp - a.timestamp || a.sessionId.localeCompare(b.sessionId));
  }
  return [...groups.values()].sort((a, b) => {
    const aLatest = a.replies[0]?.timestamp ?? 0;
    const bLatest = b.replies[0]?.timestamp ?? 0;
    return bLatest - aLatest || a.key.localeCompare(b.key);
  });
}

export function agentCardStatus(
  profile: Pick<TeammateProfile, 'status' | 'activity'> | undefined,
  pendingApproval: boolean,
): AgentCardStatus {
  if (pendingApproval) return 'waiting';
  if (!profile) return 'completed';
  if (profile.status === 'Disappeared' || profile.status === 'Suspended') return 'stopped';
  if (profile.status === 'Working' || profile.activity === 'working') return 'working';
  return 'idle';
}

/** Pane chrome consumes the same projection as Commune cards. A live roster
 * entry cannot be `completed`; map that history-only state to the neutral pane
 * state so border/highlight and card status never drift independently. */
export type AgentPaneStatus = Exclude<AgentCardStatus, 'completed'>;

export function agentPaneStatus(
  profile: Pick<TeammateProfile, 'status' | 'activity'>,
  pendingApproval: boolean,
): AgentPaneStatus {
  const status = agentCardStatus(profile, pendingApproval);
  return status === 'completed' ? 'idle' : status;
}

const STATUS_PRIORITY: readonly AgentCardStatus[] = ['waiting', 'working', 'stopped', 'idle', 'completed'];

export function aggregateAgentCardStatus(statuses: readonly AgentCardStatus[]): AgentCardStatus {
  return statuses.reduce<AgentCardStatus>(
    (best, current) => STATUS_PRIORITY.indexOf(current) < STATUS_PRIORITY.indexOf(best) ? current : best,
    'completed',
  );
}

export function agentStatusLabel(status: AgentCardStatus): string {
  switch (status) {
    case 'working': return 'Working';
    case 'waiting': return 'Waiting approval';
    case 'idle': return 'Idle';
    case 'completed': return 'Completed';
    case 'stopped': return 'Stopped';
  }
}
