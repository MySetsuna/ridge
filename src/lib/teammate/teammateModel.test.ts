/**
 * teammateModel.test.ts — tests for the front-end teammate domain mirror.
 *
 * Mirrors the defensive posture of layoutEvent.test.ts: every parser must
 * degrade gracefully on malformed input, and the risk parser must NEVER silently
 * downgrade an unknown risk (fail-closed to Dangerous so the human is asked).
 */
import { describe, it, expect } from 'vitest';
import {
  riskLabel,
  parseTopologySnapshot,
  parseHitlRequest,
  parseCircuitTripped,
  EMPTY_TOPOLOGY,
} from './teammateModel';

describe('riskLabel', () => {
  it('maps each level to its short label', () => {
    expect(riskLabel('ReadOnly')).toBe('L0');
    expect(riskLabel('WorkspaceWrite')).toBe('L1');
    expect(riskLabel('Dangerous')).toBe('L2');
  });
});

describe('parseTopologySnapshot', () => {
  it('parses a full snapshot', () => {
    const snap = parseTopologySnapshot({
      roster: [
        { id: 'a', name: 'Claude', pane_id: 'uuid-1', role: 'Leader', status: 'Working' },
        { id: 'b', pane_id: 'uuid-2', role: 'Worker', status: 'Idle' },
      ],
      leader_id: 'a',
      edges: [{ from: 'a', to: 'b', description: '跑测试' }],
    });
    expect(snap.roster).toHaveLength(2);
    expect(snap.roster[0]).toMatchObject({ id: 'a', name: 'Claude', paneId: 'uuid-1', role: 'Leader' });
    // Missing name falls back to id.
    expect(snap.roster[1].name).toBe('b');
    expect(snap.leaderId).toBe('a');
    expect(snap.edges[0]).toEqual({ from: 'a', to: 'b', description: '跑测试' });
  });

  it('drops roster entries without an id', () => {
    const snap = parseTopologySnapshot({ roster: [{ name: 'no-id' }, { id: 'ok' }] });
    expect(snap.roster.map((r) => r.id)).toEqual(['ok']);
  });

  it('coerces unknown role/status to safe defaults', () => {
    const snap = parseTopologySnapshot({ roster: [{ id: 'x', role: 'King', status: 'Vibing' }] });
    expect(snap.roster[0].role).toBe('Worker');
    expect(snap.roster[0].status).toBe('Idle');
  });

  it('maps a valid capability tier and degrades unknown/absent to undefined', () => {
    const snap = parseTopologySnapshot({
      roster: [
        { id: 'a', capability: 'Expert' },
        { id: 'b', capability: 'Wizard' },
        { id: 'c' },
      ],
    });
    expect(snap.roster[0].capability).toBe('Expert');
    expect(snap.roster[1].capability).toBeUndefined();
    expect(snap.roster[2].capability).toBeUndefined();
  });

  it('degrades to EMPTY_TOPOLOGY on garbage', () => {
    expect(parseTopologySnapshot(null)).toEqual(EMPTY_TOPOLOGY);
    expect(parseTopologySnapshot('nope')).toEqual(EMPTY_TOPOLOGY);
    expect(parseTopologySnapshot({})).toEqual(EMPTY_TOPOLOGY);
  });

  it('preserves the backend auto-discovery change signal without guessing', () => {
    expect(parseTopologySnapshot({ rosterChanged: true }).rosterChanged).toBe(true);
    expect(parseTopologySnapshot({ rosterChanged: false }).rosterChanged).toBe(false);
    expect(parseTopologySnapshot({ rosterChanged: 'true' }).rosterChanged).toBe(false);
  });
});

describe('parseHitlRequest', () => {
  it('parses a request with a bare RiskLevel string', () => {
    const req = parseHitlRequest({
      id: 'req1',
      initiator: 'pane_02',
      action: 'git push origin main',
      risk: 'Dangerous',
      reason: 'git push 推送远端',
    });
    expect(req).toEqual({
      id: 'req1',
      initiator: 'pane_02',
      action: 'git push origin main',
      level: 'Dangerous',
      reason: 'git push 推送远端',
      kind: 'approval',
    });
  });

  it('accepts a RiskAssessment object for risk', () => {
    const req = parseHitlRequest({
      id: 'r',
      action: 'rm -rf /',
      risk: { level: 'Dangerous', reason: '递归删除' },
    });
    expect(req?.level).toBe('Dangerous');
  });

  it('fails closed to Dangerous when risk is unknown/missing', () => {
    const req = parseHitlRequest({ id: 'r', action: 'mystery' });
    expect(req?.level).toBe('Dangerous');
  });

  it('returns null without an id (nothing to reply to)', () => {
    expect(parseHitlRequest({ action: 'x' })).toBeNull();
    expect(parseHitlRequest(null)).toBeNull();
  });

  it('keeps an externally reported rejection distinct from an approvable Ridge request', () => {
    const req = parseHitlRequest({
      id: 'external-1',
      kind: 'external_rejection',
      executor: 'Codex execution gateway',
      policySource: 'organization policy',
      requestId: 'request-42',
      reason: 'rejected: blocked by policy',
    });
    expect(req).toMatchObject({
      kind: 'external_rejection',
      executor: 'Codex execution gateway',
      policySource: 'organization policy',
      requestId: 'request-42',
    });
  });
});

describe('parseCircuitTripped', () => {
  it('parses a circuit-tripped payload', () => {
    const trip = parseCircuitTripped({ workspaceId: 'ws', paneId: 'uuid-2', reason: '递归/批量删除' });
    expect(trip).toEqual({ paneId: 'uuid-2', reason: '递归/批量删除' });
  });

  it('degrades empty/missing reason to a generic one', () => {
    expect(parseCircuitTripped({ paneId: 'p', reason: '' })?.reason).toBe('逻辑死锁');
    expect(parseCircuitTripped({ pane_id: 'p2' })?.reason).toBe('逻辑死锁');
  });

  it('returns null without a pane id', () => {
    expect(parseCircuitTripped({ reason: 'x' })).toBeNull();
    expect(parseCircuitTripped(null)).toBeNull();
  });
});
