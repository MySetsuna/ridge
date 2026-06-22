/**
 * teammateModel.ts — front-end typed mirror of the Rust `ridge_core::teammate`
 * domain (Domain Zero 端侧多智能体协同).
 *
 * The Tauri backend emits:
 *   - `get_teammate_topology` → a TopologySnapshot (roster + leader + edges)
 *   - event `teammate://hitl-approval-required` → a HitlRequest (Domain D2)
 *   - event `teammate://circuit-tripped` → a CircuitTrip (Domain D3)
 *
 * This module normalizes those wire shapes into typed front-end view models.
 * Every parser degrades gracefully on an unexpected payload so the Agent Center /
 * HITL UI can never break on a malformed event — same defensive posture as
 * `layoutEvent.ts`. （TML 协作审计已退场——底座化瘦身。）
 *
 * Wire enums mirror serde's default unit-variant encoding (the variant name as a
 * bare string): AgentRole/TeammateStatus/RiskLevel.
 */

// ── Enums (mirror serde unit-variant strings) ──

export type AgentRole = 'Leader' | 'Worker' | 'Observer';
export type TeammateStatus = 'Idle' | 'Working' | 'Disappeared';
export type RiskLevel = 'ReadOnly' | 'WorkspaceWrite' | 'Dangerous';

/** L0 / L1 / L2 short label for a risk level (mirrors `RiskLevel::label`). */
export function riskLabel(level: RiskLevel): 'L0' | 'L1' | 'L2' {
  switch (level) {
    case 'ReadOnly':
      return 'L0';
    case 'WorkspaceWrite':
      return 'L1';
    case 'Dangerous':
      return 'L2';
  }
}

// ── Roster / topology ──

export interface AgentCapabilities {
  readonly languageSkills: Record<string, number>;
  readonly domainSkills: readonly string[];
  readonly contextWindow: number;
}

export interface AgentPersonality {
  readonly riskTolerance: number;
  readonly thoroughness: number;
}

/** A roster entry — one teammate's front-end profile. */
export interface TeammateProfile {
  readonly id: string;
  readonly name: string;
  /** Real Ridge pane id (Uuid string), not the core's internal u32. */
  readonly paneId: string;
  readonly role: AgentRole;
  readonly status: TeammateStatus;
  readonly capabilities?: AgentCapabilities;
  readonly personality?: AgentPersonality;
}

export interface TopologyEdge {
  readonly from: string;
  readonly to: string;
  readonly description: string;
}

/** The full team snapshot rendered by the Agent Center sidebar. */
export interface TopologySnapshot {
  readonly roster: readonly TeammateProfile[];
  readonly leaderId: string | null;
  readonly edges: readonly TopologyEdge[];
}

export const EMPTY_TOPOLOGY: TopologySnapshot = {
  roster: [],
  leaderId: null,
  edges: [],
};

// ── HITL (Domain D2) ──

export interface HitlRequest {
  /** Correlation id the human's decision is sent back with. */
  readonly id: string;
  /** Who initiated the action (pane id / agent name). */
  readonly initiator: string;
  /** The raw command / method awaiting approval. */
  readonly action: string;
  readonly level: RiskLevel;
  /** Human-readable why-flagged reason from the risk classifier. */
  readonly reason: string;
}

export type HitlVerdict = 'approve' | 'reject' | 'modify';

export interface HitlDecision {
  readonly id: string;
  readonly verdict: HitlVerdict;
  /** New command text when verdict is `modify`. */
  readonly replacement?: string;
}

// ── Narrowing helpers ──

function asString(v: unknown): string | undefined {
  return typeof v === 'string' ? v : undefined;
}

function asRecord(v: unknown): Record<string, unknown> | null {
  return typeof v === 'object' && v !== null ? (v as Record<string, unknown>) : null;
}

const ROLES: ReadonlySet<string> = new Set(['Leader', 'Worker', 'Observer']);
const STATUSES: ReadonlySet<string> = new Set(['Idle', 'Working', 'Disappeared']);
const RISKS: ReadonlySet<string> = new Set(['ReadOnly', 'WorkspaceWrite', 'Dangerous']);

function asRole(v: unknown): AgentRole {
  return typeof v === 'string' && ROLES.has(v) ? (v as AgentRole) : 'Worker';
}

function asStatus(v: unknown): TeammateStatus {
  return typeof v === 'string' && STATUSES.has(v) ? (v as TeammateStatus) : 'Idle';
}

function asRisk(v: unknown): RiskLevel {
  // Accept either a bare RiskLevel string or a `{ level, reason }` RiskAssessment.
  if (typeof v === 'string' && RISKS.has(v)) return v as RiskLevel;
  const rec = asRecord(v);
  if (rec && typeof rec.level === 'string' && RISKS.has(rec.level)) {
    return rec.level as RiskLevel;
  }
  // Conservative default: treat unknown as the most-restrictive so a malformed
  // risk payload never silently auto-approves a dangerous action.
  return 'Dangerous';
}

// ── Parsers ──

/** Parse one roster entry; returns null if it lacks a usable id. */
function parseProfile(v: unknown): TeammateProfile | null {
  const rec = asRecord(v);
  if (!rec) return null;
  const id = asString(rec.id) ?? asString(rec.agentId) ?? asString(rec.agent_id);
  if (!id) return null;
  const paneId = asString(rec.paneId) ?? asString(rec.pane_id) ?? '';
  return {
    id,
    name: asString(rec.name) ?? id,
    paneId,
    role: asRole(rec.role),
    status: asStatus(rec.status),
  };
}

/**
 * Parse a `get_teammate_topology` payload into a TopologySnapshot.
 * Any unrecognized shape degrades to {@link EMPTY_TOPOLOGY}.
 */
export function parseTopologySnapshot(payload: unknown): TopologySnapshot {
  const rec = asRecord(payload);
  if (!rec) return EMPTY_TOPOLOGY;

  const rawRoster = Array.isArray(rec.roster) ? rec.roster : [];
  const roster = rawRoster
    .map(parseProfile)
    .filter((p): p is TeammateProfile => p !== null);

  const leaderId = asString(rec.leaderId) ?? asString(rec.leader_id) ?? null;

  const rawEdges = Array.isArray(rec.edges) ? rec.edges : [];
  const edges = rawEdges
    .map((e): TopologyEdge | null => {
      const er = asRecord(e);
      if (!er) return null;
      const from = asString(er.from);
      const to = asString(er.to);
      if (!from || !to) return null;
      return { from, to, description: asString(er.description) ?? '' };
    })
    .filter((e): e is TopologyEdge => e !== null);

  return { roster, leaderId, edges };
}

/**
 * Parse a `teammate://hitl-approval-required` event payload into a HitlRequest.
 * Returns null (→ caller ignores) only when there is no id to reply with; an
 * unknown/missing risk degrades to `Dangerous` so the human is always asked.
 */
export function parseHitlRequest(payload: unknown): HitlRequest | null {
  const rec = asRecord(payload);
  if (!rec) return null;
  const id = asString(rec.id) ?? asString(rec.requestId) ?? asString(rec.request_id);
  if (!id) return null;
  return {
    id,
    initiator: asString(rec.initiator) ?? '未知发起者',
    action: asString(rec.action) ?? '',
    level: asRisk(rec.risk ?? rec.level),
    reason: asString(rec.reason) ?? '',
  };
}

// ── Circuit breaker (Domain D3) ──

/** A worker that tripped the loop-breaker (from `teammate://circuit-tripped`). */
export interface CircuitTrip {
  /** Affected pane id (Uuid string). */
  readonly paneId: string;
  /** Why it tripped — the repeated-failure fingerprint surfaced by the breaker. */
  readonly reason: string;
}

/**
 * Parse a `teammate://circuit-tripped` event payload into a {@link CircuitTrip}.
 * Backend payload (circuit.rs): `{ workspaceId, paneId, reason }`. Returns null
 * without a pane id; an empty reason degrades to a generic "逻辑死锁".
 */
export function parseCircuitTripped(payload: unknown): CircuitTrip | null {
  const rec = asRecord(payload);
  if (!rec) return null;
  const paneId = asString(rec.paneId) ?? asString(rec.pane_id);
  if (!paneId) return null;
  return { paneId, reason: asString(rec.reason) || '逻辑死锁' };
}
