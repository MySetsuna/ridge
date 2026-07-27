export type AgentSyncAction =
  | { kind: 'register'; agentId: string }
  | { kind: 'release' }
  | { kind: 'none' };

interface AutoAgentState {
  agentId: string;
  syncedAt: number;
}

const autoAgents = new Map<string, AutoAgentState>();
const RESYNC_MS = 5000;

const AGENTS: readonly [string, readonly string[]][] = [
  ['claude', ['claude code', 'claude']],
  ['codex', ['openai codex', 'codex']],
  ['gemini', ['gemini cli', 'gemini']],
  ['opencode', ['opencode']],
  ['aider', ['aider']],
  ['copilot', ['github copilot', 'copilot']],
];

export function detectAgentName(...signals: Array<string | null | undefined>): string | null {
  const haystack = signals.filter(Boolean).join(' ').toLowerCase();
  for (const [agent, patterns] of AGENTS) {
    if (patterns.some((pattern) => haystack.includes(pattern))) return agent;
  }
  return null;
}

export function nextAgentSync(
  paneKey: string,
  detected: string | null,
  now = Date.now()
): AgentSyncAction {
  const current = autoAgents.get(paneKey);
  if (!detected) {
    if (!current) return { kind: 'none' };
    autoAgents.delete(paneKey);
    return { kind: 'release' };
  }
  if (!current || current.agentId !== detected || now - current.syncedAt >= RESYNC_MS) {
    autoAgents.set(paneKey, { agentId: detected, syncedAt: now });
    return { kind: 'register', agentId: detected };
  }
  return { kind: 'none' };
}

export function resetAgentSyncForTests(): void {
  autoAgents.clear();
}
