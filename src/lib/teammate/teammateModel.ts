/**
 * teammateModel.ts — front-end typed mirror of the Rust `ridge_core::teammate`
 * domain (Domain Zero 端侧多智能体协同).
 *
 * The Tauri backend emits:
 *   - `get_teammate_topology` → a TopologySnapshot (roster + leader + edges)
 *   - event `teammate://hitl-approval-required` → a HitlRequest (Domain D2)
 *
 * This module normalizes those wire shapes into typed front-end view models.
 * Every parser degrades gracefully on an unexpected payload so the Agent Center /
 * HITL UI can never break on a malformed event.
 *
 * Wire enums mirror serde's default unit-variant encoding (the variant name as a
 * bare string): AgentRole/TeammateStatus/RiskLevel.
 */

// ── Enums (mirror serde unit-variant strings) ──

export type AgentRole = 'Leader' | 'Worker' | 'Observer';
export type TeammateStatus = 'Idle' | 'Working' | 'Disappeared' | 'Suspended';
export type RiskLevel = 'ReadOnly' | 'WorkspaceWrite' | 'Dangerous';
/** Minimal auto-detected capability tier (mirrors `ridge_core::AgentTier`). */
export type AgentTier = 'Base' | 'Skilled' | 'Expert';

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

/** 活跃度（iter-62）：按该 pane 的输出流水号是否还在增长判定，与 `status`
 *  （是不是 agent pane / 有没有被暂停）正交。 */
export type AgentActivity = 'working' | 'idle';

/** A roster entry — one teammate's front-end profile. */
export interface TeammateProfile {
  readonly id: string;
  readonly name: string;
  /** Real Ridge pane id (Uuid string), not the core's internal u32. */
  readonly paneId: string;
  readonly role: AgentRole;
  readonly status: TeammateStatus;
  /** Auto-detected capability tier the Leader was elected from (optional). */
  readonly capability?: AgentTier;
  /** iter-62：由「pane 下真跑着 agent CLI」自动识别入册（而非人工标记）。 */
  readonly isAuto: boolean;
  /** iter-62：终端是否还在吐字（近 12s 有输出）。 */
  readonly activity: AgentActivity;
  /** iter-62：该 pane 输出的单调流水号（供更细的活跃度判断/去重）。 */
  readonly outputSeq: number;
  /** iter-62：近期回复——scrollback 末尾剥 ANSI 的最后几行，随快照下发
   *  （面板与手机端无需再为每个成员单独发一次 IPC）。 */
  readonly recentOutput: string;
  /** Kernel-owned identity/fencing fields; absent for legacy projections. */
  readonly sessionId?: string;
  readonly process?: DiscoveredAgent['process'];
  readonly executable?: string;
  /** Agent process cwd; authoritative for binding native history fallback. */
  readonly cwd?: string;
  readonly workspaceId?: string;
  readonly generation?: number;
  readonly lease?: string;
  readonly lifecycle?: string;
  readonly online?: boolean;
  readonly capabilities?: readonly string[];
  readonly currentDelivery?: ProtocolDelivery;
  readonly canSend?: boolean;
  readonly canCancel?: boolean;
}

export interface ProtocolDelivery {
  readonly deliveryId: string;
  readonly status?: string;
  readonly agentAcknowledged?: boolean;
  readonly deliveryAdapter?: string;
  readonly payload?: { text?: string; objective?: string };
  readonly execution?: { state?: string; reason?: string; response?: { stopReason?: string } };
}

export interface PermissionOption { readonly optionId: string; readonly name: string; readonly kind: string }

export interface TopologyEdge {
  readonly from: string;
  readonly to: string;
  readonly description: string;
}

/** The full team snapshot rendered by the Agent Center sidebar. */
export interface TopologySnapshot {
  /** OS-observed processes have no protocol membership or execution authority. */
  readonly discovered?: readonly DiscoveredAgent[];
  readonly roster: readonly TeammateProfile[];
  readonly leaderId: string | null;
  readonly edges: readonly TopologyEdge[];
  /** Backend auto-discovery changed pane↔agent projection during this snapshot. */
  readonly rosterChanged: boolean;
}

export interface DiscoveredAgent {
  readonly workspaceId: string;
  readonly paneId: string;
  readonly name: string;
  readonly process: { readonly pid: number; readonly startInstance: string };
  readonly reason: string;
  readonly terminalEpoch?: string;
  readonly recentOutput?: string;
  readonly protocolConnected?: boolean;
}

export const EMPTY_TOPOLOGY: TopologySnapshot = {
  roster: [],
  leaderId: null,
  edges: [],
  rosterChanged: false,
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
  /** Concrete execution layer; never label an unknown rejection as Ridge. */
  readonly executor?: string;
  readonly policySource?: string;
  readonly requestId?: string;
  readonly nextStep?: string;
  /** A report from another execution gateway, not a Ridge-pending approval. */
  readonly kind: 'approval' | 'external_rejection';
  readonly protocol?: 'acp';
  readonly workspaceId?: string;
  readonly agentId?: string;
  readonly generation?: number;
  readonly lease?: string;
  readonly deliveryId?: string;
  readonly groupTaskId?: string;
  readonly options?: readonly PermissionOption[];
}

/** 面板行内裁决用的**脱敏**待审批项（`list_hitl_pending` 投影，无命令全文）。 */
export interface PendingApproval {
  readonly id: string;
  readonly initiator: string;
  readonly reason: string;
  readonly resolutionNonce?: string;
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
const STATUSES: ReadonlySet<string> = new Set(['Idle', 'Working', 'Disappeared', 'Suspended']);
const RISKS: ReadonlySet<string> = new Set(['ReadOnly', 'WorkspaceWrite', 'Dangerous']);
const TIERS: ReadonlySet<string> = new Set(['Base', 'Skilled', 'Expert']);

function asRole(v: unknown): AgentRole {
  return typeof v === 'string' && ROLES.has(v) ? (v as AgentRole) : 'Worker';
}

/** Capability tier is optional metadata — absent/garbage degrades to undefined. */
function asTier(v: unknown): AgentTier | undefined {
  return typeof v === 'string' && TIERS.has(v) ? (v as AgentTier) : undefined;
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
  // 显示名自动同步 pane 标题（iter-61 用户需求）：后端 inject_roster_titles 随拓扑
  // 附带实时 OSC 标题 `title`；有则优先于注册时的静态 name。id 保持稳定不受影响。
  const title = asString(rec.title);
  const process = asRecord(asRecord(rec.protocolBinding)?.process);
  return {
    id,
    name: title?.trim() ? title : (asString(rec.name) ?? id),
    paneId,
    role: asRole(rec.role),
    status: asStatus(rec.status),
    capability: asTier(rec.capability),
    isAuto: rec.isAuto === true,
    activity: rec.activity === 'working' ? 'working' : 'idle',
    outputSeq: typeof rec.outputSeq === 'number' ? rec.outputSeq : 0,
    recentOutput: asString(rec.recentOutput) ?? '',
    sessionId: asString(rec.sessionId) ?? asString(rec.session_id),
    ...(process && Number.isSafeInteger(process.pid) && (process.pid as number) > 0 && asString(process.startInstance)
      ? { process: { pid: process.pid as number, startInstance: process.startInstance as string } } : {}),
    executable: asString(rec.executable),
    cwd: asString(rec.cwd),
    workspaceId: asString(rec.workspaceId) ?? asString(rec.workspace_id),
    generation: typeof rec.generation === 'number' ? rec.generation : undefined,
    lease: asString(rec.lease),
    lifecycle: asString(rec.lifecycle),
    online: typeof rec.online === 'boolean' ? rec.online : undefined,
    capabilities: Array.isArray(rec.capabilities)
      ? rec.capabilities.filter((item): item is string => typeof item === 'string')
      : undefined,
    currentDelivery: parseProtocolDelivery(rec.currentDelivery),
    canSend: typeof rec.canSend === 'boolean' ? rec.canSend : undefined,
    canCancel: typeof rec.canCancel === 'boolean' ? rec.canCancel : undefined,
  };
}

export function parseProtocolDelivery(value: unknown): ProtocolDelivery | undefined {
  const rec = asRecord(value);
  if (!rec || typeof rec.deliveryId !== 'string') return undefined;
  const payload = asRecord(rec.payload);
  const execution = asRecord(rec.execution);
  const response = asRecord(execution?.response);
  return { deliveryId: rec.deliveryId, status: asString(rec.status), agentAcknowledged: rec.agentAcknowledged === true,
    deliveryAdapter: asString(rec.deliveryAdapter), payload: payload ? { text: asString(payload.text), objective: asString(payload.objective) } : undefined,
    execution: execution ? { state: asString(execution.state), reason: asString(execution.reason), response: response ? { stopReason: asString(response.stopReason) } : undefined } : undefined };
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

  const discovered: DiscoveredAgent[] = Array.isArray(rec.discovered) ? rec.discovered.flatMap(value => {
    const row = asRecord(value), process = asRecord(row?.process);
    return row && process && typeof row.workspaceId === 'string' && typeof row.paneId === 'string'
      && typeof row.name === 'string' && row.connectionState === 'discovered'
      && typeof process.pid === 'number' && Number.isSafeInteger(process.pid) && process.pid > 0
      && typeof process.startInstance === 'string' && process.startInstance.length > 0
      ? [{ workspaceId: row.workspaceId, paneId: row.paneId, name: row.name,
        process: { pid: process.pid, startInstance: process.startInstance }, reason: asString(row.reason) ?? '尚未接入会话协议',
        ...(typeof row.terminalEpoch === 'string' ? { terminalEpoch: row.terminalEpoch } : {}),
        ...(typeof row.recentOutput === 'string' ? { recentOutput: row.recentOutput } : {}),
        ...(typeof row.protocolConnected === 'boolean' ? { protocolConnected: row.protocolConnected } : {}) }] : [];
  }) : [];
  return { roster, leaderId, edges, rosterChanged: rec.rosterChanged === true,
    ...(discovered.length ? { discovered } : {}) };
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
    executor: asString(rec.executor),
    policySource: asString(rec.policySource) ?? asString(rec.policy_source),
    requestId: asString(rec.requestId) ?? asString(rec.request_id),
    nextStep: asString(rec.nextStep) ?? asString(rec.next_step),
    kind: rec.kind === 'external_rejection' ? 'external_rejection' : 'approval',
    protocol: rec.protocol === 'acp' ? 'acp' : undefined,
    workspaceId: asString(rec.workspaceId), agentId: asString(rec.agentId),
    generation: typeof rec.generation === 'number' ? rec.generation : undefined, lease: asString(rec.lease),
    deliveryId: asString(rec.deliveryId), groupTaskId: asString(rec.groupTaskId),
    options: rec.protocol === 'acp' && Array.isArray(rec.options) ? rec.options.flatMap((value) => {
      const option = asRecord(value);
      return option && typeof option.optionId === 'string' && typeof option.name === 'string' && typeof option.kind === 'string'
        ? [{ optionId: option.optionId, name: option.name, kind: option.kind }] : [];
    }) : undefined,
  };
}
