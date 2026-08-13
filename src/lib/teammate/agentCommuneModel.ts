export type AgentCardStatus = 'working' | 'waiting' | 'idle' | 'completed' | 'stopped';

/** Minimal status shape shared by desktop profiles and Remote roster DTOs. */
export interface AgentStatusProfile {
  status?: string;
  activity?: string;
}

/** History is a cold, host-wide scan; never couple it to the live roster poll. */
export const AGENT_HISTORY_REFRESH_INTERVAL_MS = 5 * 60 * 1000;

export function shouldRefreshAgentHistory(lastLoadedAt: number, now = Date.now()): boolean {
  return lastLoadedAt <= 0 || now - lastLoadedAt >= AGENT_HISTORY_REFRESH_INTERVAL_MS;
}

export interface AgentHistoryReplyLike {
  agent: string;
  sessionId: string;
  timestamp: number;
}

export interface AgentReplyLookupProfile {
  id: string;
  name: string;
  sessionId?: string;
  executable?: string;
  cwd?: string;
}

function normalizeCwdIdentity(cwd: string): string {
  const normalized = cwd.trim().replaceAll('\\', '/');
  if (/^(?:[a-z]:)?\/+$/i.test(normalized)) {
    return normalized.replace(/\/+$/, '/').toLocaleLowerCase();
  }
  return normalized.replace(/\/+$/, '').toLocaleLowerCase();
}

/** Bind a live card to history only by native session id; names are fallback
 * agent-type hints, never session identity. */
export function latestReplyForProfile<T extends AgentHistoryReplyLike>(
  replies: readonly T[],
  profile: AgentReplyLookupProfile,
): T | undefined {
  const sessionId = profile.sessionId?.trim();
  const identities = new Set<string>();
  for (const raw of [profile.name, profile.id, profile.executable ?? '']) {
    const normalized = raw.trim().toLocaleLowerCase();
    if (!normalized) continue;
    identities.add(normalized);
    const executable = normalized.split(/[\\/]/).at(-1)?.replace(/\.exe$/, '');
    if (executable) identities.add(executable);
    if (normalized.startsWith('auto:')) {
      const detected = normalized.split(':')[1];
      if (detected) identities.add(detected);
    }
  }
  return replies
    .filter((reply) => {
      if (!identities.has(normalizeAgentIdentity(reply.agent))) return false;
      if (sessionId && reply.sessionId === sessionId) return true;
      if (sessionId && !sessionId.startsWith('session:')) return false;
      return !!profile.cwd && 'cwd' in reply
        && normalizeCwdIdentity(String((reply as T & { cwd?: string }).cwd ?? ''))
          === normalizeCwdIdentity(profile.cwd);
    })
    .sort((a, b) => b.timestamp - a.timestamp)[0];
}

export interface AgentHistoryGroup<T extends AgentHistoryReplyLike = AgentHistoryReplyLike> {
  key: string;
  agent: string;
  replies: T[];
}

/**
 * Reorder the persisted group list without mutating the live topology.
 * Remote controls use this same pure operation as the desktop store so an
 * optimistic tap cannot accidentally reorder a stale array in place.
 */
export function reorderAgentGroups<T extends { id: string }>(
  groups: readonly T[],
  groupId: string,
  direction: -1 | 1,
): T[] {
  const index = groups.findIndex((group) => group.id === groupId);
  const target = index + direction;
  if (index < 0 || target < 0 || target >= groups.length) return [...groups];
  const next = [...groups];
  [next[index], next[target]] = [next[target], next[index]];
  return next;
}

/** Toggle a group leader, refusing identities that are not members. */
export function toggleAgentGroupLeader<T extends { memberAgentIds: readonly string[]; leaderAgentId?: string }>(
  group: T,
  agentId: string,
): T {
  if (!group.memberAgentIds.includes(agentId)) return group;
  return {
    ...group,
    leaderAgentId: group.leaderAgentId === agentId ? undefined : agentId,
  };
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
  profile: AgentStatusProfile | undefined,
  pendingApproval: boolean,
): AgentCardStatus {
  if (pendingApproval) return 'waiting';
  if (!profile) return 'completed';
  if (profile.status === 'Disappeared' || profile.status === 'Suspended') return 'stopped';
  // Desktop topology always labels a live Agent registration `Working`; actual
  // task activity is the output-derived field. Legacy Remote DTOs without that
  // field retain their status-only behavior.
  if (profile.activity === 'working' || (profile.activity === undefined && profile.status === 'Working')) {
    return 'working';
  }
  return 'idle';
}

/** Pane chrome consumes the same projection as Commune cards. A live roster
 * entry cannot be `completed`; map that history-only state to the neutral pane
 * state so border/highlight and card status never drift independently. */
export type AgentPaneStatus = Exclude<AgentCardStatus, 'completed'>;

export type AgentAttention = 'waiting' | 'idle' | 'stopped';

export function agentPaneStatus(
  profile: AgentStatusProfile,
  pendingApproval: boolean,
): AgentPaneStatus {
  const status = agentCardStatus(profile, pendingApproval);
  return status === 'completed' ? 'idle' : status;
}

/** Attention is emitted only on a meaningful transition; initial idle is neutral. */
export function agentAttentionForTransition(
  previousStatus: AgentPaneStatus | null | undefined,
  currentStatus: AgentPaneStatus,
  pendingApproval: boolean,
  profileStatus?: string,
): AgentAttention | null {
  if (pendingApproval) return 'waiting';
  if (profileStatus === 'Disappeared') return 'stopped';
  return previousStatus === 'working' && currentStatus === 'idle' ? 'idle' : null;
}

export function agentAttentionPriority(attention: AgentAttention): number {
  if (attention === 'waiting') return 3;
  if (attention === 'stopped') return 2;
  return 1;
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
