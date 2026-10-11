import { describe, expect, it } from 'vitest';
import {
  ALL_AGENT_EVENT_KINDS,
  emptyTranscript,
  reduceEvent,
  type AgentEvent,
} from './agentEvents';

const base = emptyTranscript();

describe('agentEvents — Desktop Chat projection reducer', () => {
  it('starts empty and Discovered', () => {
    expect(base.lines).toEqual([]);
    expect(base.lifecycle).toBe('Discovered');
    expect(base.turnInFlight).toBe(false);
    expect(base.pendingApproval).toBeNull();
  });

  it('covers every canonical event kind', () => {
    expect(new Set(ALL_AGENT_EVENT_KINDS).size).toBe(ALL_AGENT_EVENT_KINDS.length);
    // Kinds defined in Rust (AgentSessionEvent) — list kept in sync with
    // packages/ridge-acp/src/events.rs.
    const rustKinds = [
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
    ];
    for (const k of rustKinds) {
      expect(ALL_AGENT_EVENT_KINDS).toContain(k);
    }
  });

  it('reduces a full happy-path sequence end-to-end', () => {
    const events: AgentEvent[] = [
      {
        kind: 'session_created',
        session_id: 's1',
        ts_unix_ms: 1000,
      },
      {
        kind: 'state_changed',
        session_id: 's1',
        lifecycle: { inner: 'Online' },
        ts_unix_ms: 1100,
      },
      {
        kind: 'turn_started',
        session_id: 's1',
        turn_id: 't1',
        prompt: 'refactor /tmp/example.txt',
        ts_unix_ms: 1200,
      },
      {
        kind: 'thinking_chunk',
        session_id: 's1',
        turn_id: 't1',
        text: 'Let me read the file first...',
        ts_unix_ms: 1300,
      },
      {
        kind: 'message_chunk',
        session_id: 's1',
        turn_id: 't1',
        text: 'I will apply a clean patch.',
        ts_unix_ms: 1400,
      },
      {
        kind: 'tool_call_started',
        session_id: 's1',
        turn_id: 't1',
        tool_call: {
          id: 'tc-1',
          name: 'read_file',
          kind: 'file_read',
          arguments: { path: '/tmp/example.txt' },
        },
        ts_unix_ms: 1500,
      },
      {
        kind: 'approval_requested',
        session_id: 's1',
        turn_id: 't1',
        tool_call_id: 'tc-1',
        reason: 'modifies a file',
        ts_unix_ms: 1600,
      },
      {
        kind: 'approval_resolved',
        session_id: 's1',
        turn_id: 't1',
        tool_call_id: 'tc-1',
        approved: true,
        ts_unix_ms: 1700,
      },
      {
        kind: 'tool_call_finished',
        session_id: 's1',
        turn_id: 't1',
        tool_call_id: 'tc-1',
        status: 'ok',
        ts_unix_ms: 1800,
      },
      {
        kind: 'turn_completed',
        session_id: 's1',
        turn_id: 't1',
        result: { phase: 'completed', message: 'done' },
        ts_unix_ms: 1900,
      },
    ];
    let state = base;
    for (const ev of events) state = reduceEvent(state, ev);

    // Lifecycle reflected from StateChanged.
    expect(state.lifecycle).toBe('Online');
    // Turn not in flight after completion.
    expect(state.turnInFlight).toBe(false);
    // No pending approval after resolution.
    expect(state.pendingApproval).toBeNull();
    // Transcript has the expected shape.
    const kinds = state.lines.map((l) => l.kind);
    expect(kinds).toContain('user_prompt');
    expect(kinds).toContain('thinking');
    expect(kinds).toContain('assistant_message');
    expect(kinds).toContain('tool_call');
    expect(kinds).toContain('approval');
    expect(kinds).toContain('turn_result');
    // Tool call status was updated to 'ok' on finish.
    const toolLine = state.lines.find((l) => l.kind === 'tool_call');
    expect(toolLine?.payload['status']).toBe('ok');
    // Approval status reflects the resolution.
    const approvalLine = state.lines.find((l) => l.kind === 'approval');
    expect(approvalLine?.payload['status']).toBe('approved');
  });

  it('coalesces adjacent chunks of the same kind+turn', () => {
    const events: AgentEvent[] = [
      {
        kind: 'turn_started',
        session_id: 's',
        turn_id: 't',
        prompt: 'p',
        ts_unix_ms: 1000,
      },
      {
        kind: 'message_chunk',
        session_id: 's',
        turn_id: 't',
        text: 'Hello, ',
        ts_unix_ms: 1100,
      },
      {
        kind: 'message_chunk',
        session_id: 's',
        turn_id: 't',
        text: 'world',
        ts_unix_ms: 1300,
      },
    ];
    let state = base;
    for (const ev of events) state = reduceEvent(state, ev);
    const msgLines = state.lines.filter((l) => l.kind === 'assistant_message');
    expect(msgLines).toHaveLength(1);
    expect(msgLines[0].payload['text']).toBe('Hello, world');
  });

  it('pendingApproval is set when an approval is requested and cleared on resolve', () => {
    const events: AgentEvent[] = [
      {
        kind: 'approval_requested',
        session_id: 's',
        turn_id: 't',
        tool_call_id: 'tc-9',
        reason: 'delete file',
        ts_unix_ms: 100,
      },
    ];
    let state = base;
    for (const ev of events) state = reduceEvent(state, ev);
    expect(state.pendingApproval).toEqual({
      tool_call_id: 'tc-9',
      reason: 'delete file',
    });

    state = reduceEvent(state, {
      kind: 'approval_resolved',
      session_id: 's',
      turn_id: 't',
      tool_call_id: 'tc-9',
      approved: false,
      ts_unix_ms: 200,
    });
    expect(state.pendingApproval).toBeNull();
  });

  it('reducer is pure (no shared mutation across calls)', () => {
    const ev: AgentEvent = {
      kind: 'turn_started',
      session_id: 's',
      turn_id: 't',
      prompt: 'p',
      ts_unix_ms: 1,
    };
    const a = reduceEvent(base, ev);
    const b = reduceEvent(base, ev);
    // Independent state objects.
    expect(a).not.toBe(b);
    expect(a.lines).not.toBe(b.lines);
    expect(a.lines[0]).not.toBe(b.lines[0]);
    expect(a.lines[0].payload).not.toBe(b.lines[0].payload);
    // Independent payload fields — mutating one doesn't bleed into the other.
    const originalText = String(b.lines[0].payload['text']);
    (a.lines[0].payload as Record<string, unknown>)['text'] = 'mutated';
    expect(String(b.lines[0].payload['text'])).toBe(originalText);
  });
});
