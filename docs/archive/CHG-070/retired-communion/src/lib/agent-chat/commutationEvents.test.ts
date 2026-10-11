// M4.1 reducer test — commutation_event rendering into ChatTranscriptState.
import { test, expect } from 'vitest';
import {
  emptyTranscript,
  reduceEvent,
  type CommutationEvent,
  type ChatTranscriptState,
} from './agentEvents';

function lastLine(state: ChatTranscriptState) {
  return state.lines[state.lines.length - 1];
}

test('staff_assigned renders a plan line', () => {
  const ev: CommutationEvent = {
    kind: 'staff_assigned',
    lead_id: 'lead-1',
    staff_id: 'staff-A',
    staff_role: 'planner',
    task_title: 'M3 task',
    ts_unix_ms: 1000,
  };
  const next = reduceEvent(emptyTranscript(), ev);
  const line = lastLine(next);
  expect(line.kind).toBe('plan');
  expect(line.payload['staff_role']).toBe('planner');
  expect(line.payload['staff_id']).toBe('staff-A');
  expect(line.payload['title']).toBe('M3 task');
});

test('handoff renders a plan line with from/to', () => {
  const ev: CommutationEvent = {
    kind: 'handoff',
    lead_id: 'lead-1',
    from_staff_id: 'staff-A',
    to_staff_id: 'staff-B',
    turn_id: 'handoff-1',
    ts_unix_ms: 2000,
  };
  const next = reduceEvent(emptyTranscript(), ev);
  const line = lastLine(next);
  expect(line.payload['from_staff_id']).toBe('staff-A');
  expect(line.payload['to_staff_id']).toBe('staff-B');
});

test('staff_progress marks turnInFlight', () => {
  const ev: CommutationEvent = {
    kind: 'staff_progress',
    lead_id: 'lead-1',
    staff_id: 'staff-A',
    turn_id: 't-1',
    ts_unix_ms: 3000,
  };
  const next = reduceEvent(emptyTranscript(), ev);
  expect(next.turnInFlight).toBe(true);
  const line = lastLine(next);
  expect(line.kind).toBe('tool_call');
  expect(line.payload['tool_call_id']).toBe('staff-A');
});

test('staff_completed clears the matching tool_call line', () => {
  const startEv: CommutationEvent = {
    kind: 'staff_progress',
    lead_id: 'lead-1',
    staff_id: 'staff-A',
    turn_id: 't-1',
    ts_unix_ms: 3000,
  };
  const afterStart = reduceEvent(emptyTranscript(), startEv);
  const doneEv: CommutationEvent = {
    kind: 'staff_completed',
    lead_id: 'lead-1',
    staff_id: 'staff-A',
    turn_id: 't-1',
    ts_unix_ms: 4000,
  };
  const afterDone = reduceEvent(afterStart, doneEv);
  expect(afterDone.turnInFlight).toBe(false);
  const toolLine = afterDone.lines.find((l) => l.payload['tool_call_id'] === 'staff-A');
  expect(toolLine?.payload['status']).toBe('completed');
});

test('commutation_completed is the final turn_result line', () => {
  const ev: CommutationEvent = {
    kind: 'commutation_completed',
    lead_id: 'lead-1',
    summary: 'all staff handoff finished',
    ts_unix_ms: 9000,
  };
  const next = reduceEvent(emptyTranscript(), ev);
  expect(next.turnInFlight).toBe(false);
  const line = lastLine(next);
  expect(line.kind).toBe('turn_result');
  expect(line.payload['phase']).toBe('completed');
  expect(line.payload['message']).toBe('all staff handoff finished');
});

test('commutation_failed surfaces failure to transcript', () => {
  const ev: CommutationEvent = {
    kind: 'commutation_failed',
    lead_id: 'lead-1',
    reason: 'staff prompt error',
    ts_unix_ms: 9000,
  };
  const next = reduceEvent(emptyTranscript(), ev);
  const line = lastLine(next);
  expect(line.payload['status']).toBe('failed');
  expect(line.payload['message']).toBe('staff prompt error');
});
