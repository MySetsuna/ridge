// M4.2 — Provider restore verification.
// Simulates what the Desktop UI does after `acp_session_load`:
// 1. Take a ProviderSpec (provider/model/effort)
// 2. Take a snapshot (agent-chat panel state)
// 3. Restore: assert ProviderSpec from snapshot drives re-open
//
// This test guards against drift where the UI would maintain a second
// provider truth outside AgentSession.
import { test, expect } from 'vitest';
import { reduceEvent, type AgentEvent, type ChatTranscriptState, emptyTranscript } from './agentEvents';
import { createSnapshot, restoreFromSnapshot } from './providerRestore';
import type { PersistedSessionShape } from './providerRestore';

test('ProviderSpec round-trips through snapshot without second source of truth', () => {
  const original: PersistedSessionShape = {
    session_id: 'sess-1',
    provider: {
      kind: 'opencode_acp',
      model: { id: 'claude-sonnet-4-5', tier: 'skilled', effort: 'high' },
      config: {},
    },
  };

  const snapshot = createSnapshot(original);
  const restored = restoreFromSnapshot(snapshot);

  expect(restored.session_id).toBe(original.session_id);
  expect(restored.provider.kind).toBe('opencode_acp');
  expect(restored.provider.model.id).toBe('claude-sonnet-4-5');
  expect(restored.provider.model.effort).toBe('high');
});

test('Snapshot preserves effort across restore (no default fallback)', () => {
  const original: PersistedSessionShape = {
    session_id: 'sess-2',
    provider: {
      kind: 'opencode_acp',
      model: { id: 'gpt-4o', tier: 'skilled', effort: 'low' },
      config: {},
    },
  };
  const snapshot = createSnapshot(original);
  const restored = restoreFromSnapshot(snapshot);
  expect(restored.provider.model.effort).toBe('low');
});

test('Provider picker persistence — round-trip preserves Mock default', () => {
  const original: PersistedSessionShape = {
    session_id: 'sess-3',
    provider: {
      kind: 'mock',
      model: { id: 'mock-claude-haiku', tier: 'base' },
      config: {},
    },
  };
  const snapshot = createSnapshot(original);
  const restored = restoreFromSnapshot(snapshot);
  expect(restored.provider.kind).toBe('mock');
  expect(restored.provider.model.id).toBe('mock-claude-haiku');
});
