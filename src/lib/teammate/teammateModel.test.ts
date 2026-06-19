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
  humanizeTmlAction,
  parseTmlMessage,
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

  it('degrades to EMPTY_TOPOLOGY on garbage', () => {
    expect(parseTopologySnapshot(null)).toEqual(EMPTY_TOPOLOGY);
    expect(parseTopologySnapshot('nope')).toEqual(EMPTY_TOPOLOGY);
    expect(parseTopologySnapshot({})).toEqual(EMPTY_TOPOLOGY);
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
});

describe('humanizeTmlAction', () => {
  it('renders each action kind', () => {
    expect(humanizeTmlAction('AssignTask', 'Claude', 'Hermes', { objective: '跑单测' })).toContain(
      'Claude 给 Hermes 派活：跑单测'
    );
    expect(humanizeTmlAction('YieldControl', 'A', 'B', { reason: '挂起' })).toContain('控制权');
    expect(humanizeTmlAction('ReportStatus', 'B', 'A', { status: 'PASS' })).toContain('汇报：PASS');
    expect(humanizeTmlAction('PeerTalk', 'A', 'B')).toContain('A 对 B 说话');
  });

  it('falls back when payload fields are missing', () => {
    expect(humanizeTmlAction('AssignTask', 'A', 'B')).toContain('一个任务');
  });
});

describe('parseTmlMessage', () => {
  it('parses a header-wrapped TML message into a humanized audit entry', () => {
    const entry = parseTmlMessage(
      {
        header: {
          from_pane: 'p1',
          to_pane: 'p2',
          action: { type: 'AssignTask', payload: { objective: '重构缓存' } },
        },
        body: 'go',
      },
      (id) => (id === 'p1' ? 'Claude' : 'Hermes')
    );
    expect(entry).not.toBeNull();
    expect(entry?.kind).toBe('AssignTask');
    expect(entry?.fromPane).toBe('p1');
    expect(entry?.text).toBe('Claude 给 Hermes 派活：重构缓存');
  });

  it('defaults unknown action kind to PeerTalk', () => {
    const entry = parseTmlMessage({ header: { from_pane: 'p1', to_pane: 'p2', action: { type: 'Bogus' } } });
    expect(entry?.kind).toBe('PeerTalk');
    expect(entry?.text).toBe('p1 对 p2 说话');
  });

  it('returns null on garbage', () => {
    expect(parseTmlMessage(null)).toBeNull();
    expect(parseTmlMessage(42)).toBeNull();
  });
});
