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
  cwd?: string;
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

const CANONICAL_AGENT_NAMES: Readonly<Record<string, string>> = {
  'claude-code': 'claude',
  'codex-cli': 'codex',
  'gemini-cli': 'gemini',
};

/** Return stable aliases for process names, auto ids, and persisted JSONL labels. */
export function agentIdentityAliases(agent: string): string[] {
  const aliases = new Set<string>();
  const add = (raw: string): void => {
    const normalized = raw.trim().replaceAll('\\', '/').toLocaleLowerCase();
    if (!normalized) return;
    aliases.add(normalized);
    const basename = normalized.split('/').at(-1) ?? normalized;
    aliases.add(basename);
    const stem = basename.replace(/\.(?:exe|cmd|bat|ps1|js|cjs|mjs)$/i, '');
    aliases.add(stem);
    if (normalized.startsWith('auto:')) {
      const detected = normalized.split(':')[1];
      if (detected) add(detected);
    }
  };
  add(agent);
  return [...aliases];
}

/** Stable display-independent identity used for history grouping and lookup. */
export function normalizeAgentIdentity(agent: string): string {
  for (const alias of agentIdentityAliases(agent)) {
    const canonical = CANONICAL_AGENT_NAMES[alias];
    if (canonical) return canonical;
    if (['claude', 'codex', 'grok', 'gemini', 'cursor-agent', 'aider'].includes(alias)) {
      return alias;
    }
  }
  return agentIdentityAliases(agent)[0] ?? 'unknown';
}

function historyReplyMatchesAgent<T extends AgentHistoryReplyLike>(
  reply: T,
  profile: AgentReplyLookupProfile,
): boolean {
  const profileIdentities = new Set(
    [profile.name, profile.id, profile.executable ?? ''].flatMap(agentIdentityAliases),
  );
  return agentIdentityAliases(reply.agent).some((alias) => profileIdentities.has(alias));
}

/** Match a persisted reply to a live profile without confusing same-type agents.
 * Native ids win; synthetic kernel ids fall back to the pane cwd. */
export function historyReplyMatchesProfile<T extends AgentHistoryReplyLike>(
  reply: T,
  profile: AgentReplyLookupProfile,
): boolean {
  if (!historyReplyMatchesAgent(reply, profile)) return false;
  const sessionId = profile.sessionId?.trim();
  if (sessionId && reply.sessionId === sessionId) return true;
  if (sessionId && !sessionId.startsWith('session:')) return false;
  return !!profile.cwd && !!reply.cwd
    && normalizeCwdIdentity(reply.cwd) === normalizeCwdIdentity(profile.cwd);
}

/** Bind a live card to history by native session id, with a cwd fallback for
 * synthetic kernel session ids and launchers that do not expose resume ids. */
export function latestReplyForProfile<T extends AgentHistoryReplyLike>(
  replies: readonly T[],
  profile: AgentReplyLookupProfile,
): T | undefined {
  return replies
    .filter((reply) => historyReplyMatchesProfile(reply, profile))
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

export function sortMembersBySessionId<T extends { profile: { sessionId?: string; paneId?: string; id: string } }>(
  members: readonly T[],
): T[] {
  return [...members].sort((a, b) => {
    const left = a.profile.sessionId?.trim() || a.profile.paneId || a.profile.id;
    const right = b.profile.sessionId?.trim() || b.profile.paneId || b.profile.id;
    return left.localeCompare(right);
  });
}

/** Latch unread attention until focus. Remount/poll must not restart a flash. */
export function latchAgentAttention(input: {
  previousStatus: AgentPaneStatus | null | undefined;
  currentStatus: AgentPaneStatus;
  pending: boolean;
  profileStatus?: string;
  outputSeq: number;
  existingAttention?: AgentAttention | null;
  seenBefore: boolean;
}): AgentAttention | null {
  let signal = agentAttentionForTransition(
    input.previousStatus,
    input.currentStatus,
    input.pending,
    input.profileStatus,
  );
  if (signal === null && !input.seenBefore && input.currentStatus === 'idle' && input.outputSeq > 0) {
    signal = 'idle';
  }
  const existing = input.existingAttention ?? null;
  if (signal === null) return existing;
  if (!existing || agentAttentionPriority(signal) > agentAttentionPriority(existing)) return signal;
  return existing;
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
    case 'stopped': return 'Offline';
  }
}
