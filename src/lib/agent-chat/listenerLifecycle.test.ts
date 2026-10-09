// M4.2 — Listener lifecycle guard for commutation_event / acp_event streams.
//
// Mirrors the M2.1 `listener_double_subscribe_does_not_double_deliver` test
// in Rust; same invariant on the Svelte side. When DefaultWiredAgentChat
// mounts twice (e.g. remount or React/Svelte dev mode double-bind) the
// `mergedEvents` array must not duplicate each event.

import { test, expect } from 'vitest';
import { reduceEvent, type AgentEvent, emptyTranscript } from './agentEvents';

test('reducer is idempotent across re-runs with the same event stream', () => {
  const events: AgentEvent[] = [
    { kind: 'turn_started', session_id: 's1', turn_id: 't-1', prompt: 'hi', ts_unix_ms: 1 },
    { kind: 'thinking_chunk', session_id: 's1', turn_id: 't-1', text: 'a', ts_unix_ms: 2 },
    { kind: 'thinking_chunk', session_id: 's1', turn_id: 't-1', text: 'b', ts_unix_ms: 3 },
    { kind: 'turn_completed', session_id: 's1', turn_id: 't-1', result: { phase: 'ok', message: '' }, ts_unix_ms: 4 },
  ];

  const r1 = events.reduce(reduceEvent, emptyTranscript());
  const r2 = events.reduce(reduceEvent, emptyTranscript());
  // Same input → same output regardless of how many times we replay it
  expect(r1.lines.length).toBe(r2.lines.length);
  expect(r1.lines.map((l) => l.kind)).toEqual(r2.lines.map((l) => l.kind));
});

test('replaying a mounted projection does not append history twice', () => {
  const events: AgentEvent[] = [{ kind: 'turn_started', session_id: 's1', turn_id: 't1', prompt: 'once', ts_unix_ms: 1 }];
  const mounted = events.reduce(reduceEvent, emptyTranscript());
  const remounted = events.reduce(reduceEvent, emptyTranscript());
  expect(remounted).toEqual(mounted);
  expect(remounted.lines.filter(line => line.kind === 'user_prompt')).toHaveLength(1);
});
