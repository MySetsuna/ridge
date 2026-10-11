// Canonical event shape for the Agent Chat projection.
//
// Mirrors `ridge_acp::events::AgentSessionEvent` (Rust) so a Svelte consumer
// can render the chat surface WITHOUT importing Tauri, shell, or platform
// types. The shape is the contract; this file owns it on the Svelte side.
//
// This module is intentionally pure types + a small reducer. The Desktop
// Chat (`AgentChatPanel.svelte`) is a projection that takes a feed of these
// events and renders it. It does NOT own session state — AgentSession is
// the single source of truth.

/** Tagged event kinds, kept in sync with the Rust enum's serde `kind` tag. */
export type AgentEvent =
  | {
      kind: 'session_created';
      session_id: string;
      ts_unix_ms: number;
    }
  | {
      kind: 'state_changed';
      session_id: string;
      lifecycle: { inner: string };
      ts_unix_ms: number;
    }
  | {
      kind: 'turn_started';
      session_id: string;
      turn_id: string;
      prompt: string;
      ts_unix_ms: number;
    }
  | {
      kind: 'thinking_chunk';
      session_id: string;
      turn_id: string;
      text: string;
      ts_unix_ms: number;
    }
  | {
      kind: 'message_chunk';
      session_id: string;
      turn_id: string;
      text: string;
      ts_unix_ms: number;
    }
  | {
      kind: 'tool_call_started';
      session_id: string;
      turn_id: string;
      tool_call: {
        id: string;
        name: string;
        kind: string;
        arguments: Record<string, unknown>;
      };
      ts_unix_ms: number;
    }
  | {
      kind: 'tool_call_finished';
      session_id: string;
      turn_id: string;
      tool_call_id: string;
      status: string;
      ts_unix_ms: number;
    }
  | {
      kind: 'approval_requested';
      session_id: string;
      turn_id: string;
      tool_call_id: string;
      reason: string;
      ts_unix_ms: number;
    }
  | {
      kind: 'approval_resolved';
      session_id: string;
      turn_id: string;
      tool_call_id: string;
      approved: boolean;
      ts_unix_ms: number;
    }
  | {
      kind: 'plan_changed';
      session_id: string;
      turn_id: string;
      plan: { entries: Array<{ id: string; title: string; status: string }> };
      ts_unix_ms: number;
    }
  | {
      kind: 'turn_completed';
      session_id: string;
      turn_id: string;
      result: { phase: string; message: string };
      ts_unix_ms: number;
    }
  | {
      kind: 'turn_failed';
      session_id: string;
      turn_id: string;
      error: { code: number; message: string };
      ts_unix_ms: number;
    }
  | {
      kind: 'session_completed';
      session_id: string;
      ts_unix_ms: number;
    }
  | {
      kind: 'session_failed';
      session_id: string;
      error: { code: number; message: string };
      ts_unix_ms: number;
    };

/** Stable kind set — exposed so consumers can switch exhaustively. */
export type AgentEventKind = AgentEvent['kind'];

export const ALL_AGENT_EVENT_KINDS: readonly AgentEventKind[] = [
  'session_created',
  'state_changed',
  'turn_started',
  'thinking_chunk',
  'message_chunk',
  'tool_call_started',
  'tool_call_finished',
  'approval_requested',
  'approval_resolved',
  'plan_changed',
  'turn_completed',
  'turn_failed',
  'session_completed',
  'session_failed',
] as const;

// M4.1: CommutationEvent shape mirrors `ridge_acp::communion::CommutationEvent`.
// Single chat (M3 §4) renders BOTH AgentSessionEvent and CommutationEvent
// without holding a second source of session/task truth.
//
// Each variant carries enough info for the projection to render a line:
// - lead_id: which conversation (matches AgentSessionEvent.session_id)
// - ts_unix_ms: ordering
// - per-variant fields: title / staff_role / context / reason / etc.
export type CommutationEventKind =
  | 'created'
  | 'staff_assigned'
  | 'staff_progress'
  | 'staff_completed'
  | 'staff_failed'
  | 'handoff'
  | 'commutation_completed'
  | 'commutation_failed'
  | 'final_delivered';

export type CommutationEvent =
  | {
      kind: 'created';
      lead_id: string;
      staff_roles: string[];
      task_title: string;
      task_context: string;
      ts_unix_ms: number;
    }
  | {
      kind: 'staff_assigned';
      lead_id: string;
      staff_id: string;
      staff_role: string;
      task_title: string;
      ts_unix_ms: number;
    }
  | {
      kind: 'staff_progress';
      lead_id: string;
      staff_id: string;
      turn_id: string;
      ts_unix_ms: number;
    }
  | {
      kind: 'staff_completed';
      lead_id: string;
      staff_id: string;
      turn_id: string;
      ts_unix_ms: number;
    }
  | {
      kind: 'staff_failed';
      lead_id: string;
      staff_id: string;
      turn_id: string;
      reason: string;
      ts_unix_ms: number;
    }
  | {
      kind: 'handoff';
      lead_id: string;
      from_staff_id: string;
      to_staff_id: string;
      turn_id: string;
      ts_unix_ms: number;
    }
  | {
      kind: 'commutation_completed';
      lead_id: string;
      summary: string;
      ts_unix_ms: number;
    }
  | {
      kind: 'commutation_failed';
      lead_id: string;
      reason: string;
      ts_unix_ms: number;
    }
  | {
      kind: 'final_delivered';
      lead_id: string;
      ts_unix_ms: number;
    };

export const ALL_COMMUTATION_EVENT_KINDS: readonly CommutationEventKind[] = [
  'created',
  'staff_assigned',
  'staff_progress',
  'staff_completed',
  'staff_failed',
  'handoff',
  'commutation_completed',
  'commutation_failed',
  'final_delivered',
] as const;

/**
 * One rendered line in the chat transcript. The projection component
 * renders a stream of these. Built by reducing the canonical event feed.
 */
export interface TranscriptLine {
  /** Discriminator for the renderer (NOT a platform-bound type). */
  kind:
    | 'user_prompt'
    | 'thinking'
    | 'assistant_message'
    | 'tool_call'
    | 'approval'
    | 'turn_result'
    | 'plan'
    | 'session_state';
  /** Stable id for `key={...}` in Svelte. */
  id: string;
  ts_unix_ms: number;
  /** Optional sub-structure, lazily consumed by the renderer. */
  payload: Readonly<Record<string, unknown>>;
}

export interface ChatTranscriptState {
  lines: TranscriptLine[];
  /** Lifecycle view of the active session — kept here for status indicators. */
  lifecycle: string;
  /** True when a turn is in flight (turn_started without turn_completed/failed). */
  turnInFlight: boolean;
  /** True when an approval is pending for the user. */
  pendingApproval: { tool_call_id: string; reason: string } | null;
  pendingTargets?: ApprovalTarget[];
}

// Combined reducer: AgentEvent OR CommutationEvent → ChatTranscriptState.
// Single chat renders both event streams without holding a second source of
// session/task truth (M3 §4 + M4.1 §3).
export function reduceEvent(state: ChatTranscriptState, ev: AgentEvent | CommutationEvent): ChatTranscriptState {
  // Dispatch CommutationEvent to its dedicated reducer; AgentEvent handled below.
  if (
    ev.kind === 'created' ||
    ev.kind === 'staff_assigned' ||
    ev.kind === 'staff_progress' ||
    ev.kind === 'staff_completed' ||
    ev.kind === 'staff_failed' ||
    ev.kind === 'handoff' ||
    ev.kind === 'commutation_completed' ||
    ev.kind === 'commutation_failed' ||
    ev.kind === 'final_delivered'
  ) {
    return reduceCommutationEvent(state, ev);
  }
  switch (ev.kind) {
    case 'session_created':
      return {
        ...state,
        lines: [
          ...state.lines,
          {
            kind: 'session_state',
            id: `${ev.ts_unix_ms}:session_created:${ev.session_id}`,
            ts_unix_ms: ev.ts_unix_ms,
            payload: { lifecycle: 'created' },
          },
        ],
      };
    case 'state_changed':
      return {
        ...state,
        lifecycle: ev.lifecycle.inner,
        lines: [
          ...state.lines,
          {
            kind: 'session_state',
            id: `${ev.ts_unix_ms}:state:${ev.lifecycle.inner}`,
            ts_unix_ms: ev.ts_unix_ms,
            payload: { lifecycle: ev.lifecycle.inner },
          },
        ],
      };
    case 'turn_started':
      return {
        ...state,
        turnInFlight: true,
        lines: [
          ...state.lines,
          {
            kind: 'user_prompt',
            id: `${ev.ts_unix_ms}:turn:${ev.turn_id}:prompt`,
            ts_unix_ms: ev.ts_unix_ms,
            payload: { text: ev.prompt, turn_id: ev.turn_id },
          },
        ],
      };
    case 'thinking_chunk':
      return appendChunk(state, ev.ts_unix_ms, ev.turn_id, 'thinking', ev.text);
    case 'message_chunk':
      return appendChunk(state, ev.ts_unix_ms, ev.turn_id, 'assistant_message', ev.text);
    case 'tool_call_started':
      return {
        ...state,
        lines: [
          ...state.lines,
          {
            kind: 'tool_call',
            id: `${ev.session_id}:${ev.turn_id}:${ev.ts_unix_ms}:tool:${ev.tool_call.id}:start`,
            ts_unix_ms: ev.ts_unix_ms,
            payload: {
              session_id: ev.session_id, turn_id: ev.turn_id, tool_call_id: ev.tool_call.id,
              name: ev.tool_call.name,
              status: 'running',
              arguments: ev.tool_call.arguments,
            },
          },
        ],
      };
    case 'tool_call_finished':
      return {
        ...state,
        lines: state.lines.map((l) =>
          l.kind === 'tool_call' && l.payload['tool_call_id'] === ev.tool_call_id && l.payload['turn_id'] === ev.turn_id
            ? {
                ...l,
                payload: { ...l.payload, status: ev.status },
                id: `${ev.session_id}:${ev.turn_id}:${ev.ts_unix_ms}:tool:${ev.tool_call_id}:finished`,
              }
            : l,
        ),
      };
    case 'approval_requested':
      return {
        ...state,
        pendingApproval: { tool_call_id: ev.tool_call_id, reason: ev.reason },
        pendingTargets: [...(state.pendingTargets ?? []), { tool_call_id: ev.tool_call_id, reason: ev.reason, session_id: ev.session_id, turn_id: ev.turn_id }],
        lines: [
          ...state.lines,
          {
            kind: 'approval',
            id: `${ev.session_id}:${ev.turn_id}:${ev.ts_unix_ms}:approval:${ev.tool_call_id}:request`,
            ts_unix_ms: ev.ts_unix_ms,
            payload: {
              session_id: ev.session_id, turn_id: ev.turn_id, tool_call_id: ev.tool_call_id,
              reason: ev.reason,
              status: 'requested',
            },
          },
        ],
      };
    case 'approval_resolved':
      return {
        ...state,
        pendingTargets: (state.pendingTargets ?? []).filter(target => target.turn_id !== ev.turn_id || target.tool_call_id !== ev.tool_call_id),
        pendingApproval: null,
        lines: state.lines.map((l) =>
          l.kind === 'approval' && l.payload['tool_call_id'] === ev.tool_call_id && l.payload['turn_id'] === ev.turn_id
            ? {
                ...l,
                payload: { ...l.payload, status: ev.approved ? 'approved' : 'denied' },
                id: `${ev.session_id}:${ev.turn_id}:${ev.ts_unix_ms}:approval:${ev.tool_call_id}:resolved`,
              }
            : l,
        ),
      };
    case 'plan_changed':
      return {
        ...state,
        lines: [
          ...state.lines,
          {
            kind: 'plan',
            id: `${ev.ts_unix_ms}:plan:${ev.turn_id}`,
            ts_unix_ms: ev.ts_unix_ms,
            payload: { entries: ev.plan.entries },
          },
        ],
      };
    case 'turn_completed':
      return {
        ...state,
        pendingApproval: null,
        pendingTargets: (state.pendingTargets ?? []).filter(target => target.turn_id !== ev.turn_id),
        turnInFlight: false,
        lines: [
          ...state.lines,
          {
            kind: 'turn_result',
            id: `${ev.ts_unix_ms}:turn:${ev.turn_id}:completed`,
            ts_unix_ms: ev.ts_unix_ms,
            payload: { phase: ev.result.phase, message: ev.result.message, status: 'completed' },
          },
        ],
      };
    case 'turn_failed':
      return {
        ...state,
        pendingApproval: null,
        pendingTargets: (state.pendingTargets ?? []).filter(target => target.turn_id !== ev.turn_id),
        turnInFlight: false,
        lines: [
          ...state.lines,
          {
            kind: 'turn_result',
            id: `${ev.ts_unix_ms}:turn:${ev.turn_id}:failed`,
            ts_unix_ms: ev.ts_unix_ms,
            payload: {
              phase: 'failed',
              message: ev.error.message,
              status: 'failed',
              code: ev.error.code,
            },
          },
        ],
      };
    case 'session_completed':
    case 'session_failed':
      return { ...state, pendingTargets: (state.pendingTargets ?? []).filter(target => target.session_id !== ev.session_id), pendingApproval: null, turnInFlight: false };
  }
}

// M4.1: reduceCommutationEvent renders CommutationEvent into the same
// ChatTranscriptState. Single chat (M3 §4) shows Lead Plan / StaffAssigned /
// StaffProgress / Handoff / LeadFinal without holding a second task truth.
export function reduceCommutationEvent(
  state: ChatTranscriptState,
  ev: CommutationEvent,
): ChatTranscriptState {
  switch (ev.kind) {
    case 'created':
      return {
        ...state,
        lines: [
          ...state.lines,
          {
            kind: 'plan',
            id: `${ev.ts_unix_ms}:commutation:created:${ev.lead_id}`,
            ts_unix_ms: ev.ts_unix_ms,
            payload: {
              title: ev.task_title,
              context: ev.task_context,
              staff_roles: ev.staff_roles,
              status: 'created',
            },
          },
        ],
      };
    case 'staff_assigned':
      return {
        ...state,
        lines: [
          ...state.lines,
          {
            kind: 'plan',
            id: `${ev.ts_unix_ms}:commutation:assigned:${ev.staff_id}`,
            ts_unix_ms: ev.ts_unix_ms,
            payload: {
              title: ev.task_title,
              staff_role: ev.staff_role,
              staff_id: ev.staff_id,
              status: 'assigned',
            },
          },
        ],
      };
    case 'staff_progress':
      return {
        ...state,
        turnInFlight: true,
        lines: [
          ...state.lines,
          {
            kind: 'tool_call',
            id: `${ev.ts_unix_ms}:commutation:progress:${ev.turn_id}`,
            ts_unix_ms: ev.ts_unix_ms,
            payload: {
              tool_call_id: ev.staff_id,
              name: `staff:${ev.staff_id}:turn:${ev.turn_id}`,
              status: 'running',
            },
          },
        ],
      };
    case 'staff_completed':
      return {
        ...state,
        turnInFlight: false,
        lines: state.lines.map((l) =>
          l.kind === 'tool_call' && l.payload['tool_call_id'] === ev.staff_id
            ? {
                ...l,
                payload: { ...l.payload, status: 'completed' },
              }
            : l,
        ),
      };
    case 'staff_failed':
      return {
        ...state,
        lines: state.lines.map((l) =>
          l.kind === 'tool_call' && l.payload['tool_call_id'] === ev.staff_id
            ? {
                ...l,
                payload: { ...l.payload, status: 'failed', reason: ev.reason },
              }
            : l,
        ),
      };
    case 'handoff':
      return {
        ...state,
        lines: [
          ...state.lines,
          {
            kind: 'plan',
            id: `${ev.ts_unix_ms}:commutation:handoff:${ev.turn_id}`,
            ts_unix_ms: ev.ts_unix_ms,
            payload: {
              from_staff_id: ev.from_staff_id,
              to_staff_id: ev.to_staff_id,
              turn_id: ev.turn_id,
              status: 'handoff',
            },
          },
        ],
      };
    case 'commutation_completed':
      return {
        ...state,
        turnInFlight: false,
        lines: [
          ...state.lines,
          {
            kind: 'turn_result',
            id: `${ev.ts_unix_ms}:commutation:completed:${ev.lead_id}`,
            ts_unix_ms: ev.ts_unix_ms,
            payload: {
              phase: 'completed',
              message: ev.summary,
              status: 'completed',
            },
          },
        ],
      };
    case 'commutation_failed':
      return {
        ...state,
        turnInFlight: false,
        lines: [
          ...state.lines,
          {
            kind: 'turn_result',
            id: `${ev.ts_unix_ms}:commutation:failed:${ev.lead_id}`,
            ts_unix_ms: ev.ts_unix_ms,
            payload: {
              phase: 'failed',
              message: ev.reason,
              status: 'failed',
            },
          },
        ],
      };
    case 'final_delivered':
      // Lightweight status update; no transcript side-effect.
      return state;
  }
}

/** Append a streaming chunk; coalesce with the previous line if same kind+turn. */
function appendChunk(
  state: ChatTranscriptState,
  ts: number,
  turn_id: string,
  kind: 'thinking' | 'assistant_message',
  text: string,
): ChatTranscriptState {
  const last = state.lines[state.lines.length - 1];
  if (
    last &&
    last.kind === kind &&
    last.payload['turn_id'] === turn_id &&
    ts - last.ts_unix_ms < 1500
  ) {
    return {
      ...state,
      lines: state.lines.map((l, i) =>
        i === state.lines.length - 1
          ? {
              ...l,
              payload: { ...l.payload, text: `${String(l.payload['text'] ?? '')}${text}` },
              ts_unix_ms: ts,
            }
          : l,
      ),
    };
  }
  return {
    ...state,
    lines: [
      ...state.lines,
      {
        kind,
        id: `${ts}:${kind}:${turn_id}`,
        ts_unix_ms: ts,
        payload: { turn_id, text },
      },
    ],
  };
}

export function emptyTranscript(): ChatTranscriptState {
  return {
    lines: [],
    lifecycle: 'Discovered',
    turnInFlight: false,
    pendingApproval: null,
  };
}

// Read-only server projections. Forms issue commands; only PlanStore mutates tasks.
export type ProviderInput = { kind: 'mock' | 'anthropic' | 'open_ai' | 'opencode_acp'; model: { id: string; tier: string; effort?: 'low' | 'medium' | 'high' }; config?: Record<string, unknown> };
export type TaskProjection = { id: string; title: string; description: string; point: 'small' | 'medium' | 'large' | 'extra_large'; dependencies: string[]; assignee: string | null; assigned_session_id: string | null; status: string; result: { status: string; handoff: { summary: string; artifacts: string[]; diff_check_status: string; notes: string[] } } | null };
export type PlanProjection = { id: string; title: string; confirmed: boolean; lead_provider?: ProviderInput | null; staff: Record<string, ProviderInput>; stories: { id: string; title: string; tasks: TaskProjection[] }[] };
export type LeadOutcome = { plan_id: string; state: string; task_outcomes: { task_id: string; title: string; staff_session_id: string | null; status: string; handoff: { summary: string; artifacts: string[]; diff_check_status: string; notes: string[] } }[]; failed_or_blocked: string[]; summary: string };
export type WorkflowProjectionEvent = { kind: 'plan_updated'; plan: PlanProjection } | { kind: 'agent_event'; plan_id: string; task_id: string; event: AgentEvent } | { kind: 'lead_final'; outcome: LeadOutcome };

export type ApprovalTarget = { session_id: string; turn_id: string; tool_call_id: string; reason: string };

export type StaffPatch = { kind?: ProviderInput['kind']; model?: Partial<ProviderInput['model']> };
