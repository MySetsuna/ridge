import { describe, expect, it } from 'vitest';
import {
  compositionAllGreen,
  compositionReport,
  runAllCompositionScenarios,
  scenarioGitOrch,
  scenarioOutboundHistory,
  scenarioProtocolLink,
} from './compositionHarness';

describe('compositionHarness (C59)', () => {
  it('all product scenarios green', () => {
    const all = runAllCompositionScenarios();
    for (const s of all) {
      expect(s.ok, `${s.name}: ${s.failures.join(',')}`).toBe(true);
    }
    expect(compositionAllGreen()).toBe(true);
  });

  it('outbound history detach path', () => {
    expect(scenarioOutboundHistory().ok).toBe(true);
  });

  it('protocol blocks desktop host methods', () => {
    expect(scenarioProtocolLink().ok).toBe(true);
  });

  it('git+orch critical/degraded', () => {
    expect(scenarioGitOrch().ok).toBe(true);
  });

  it('report is non-empty', () => {
    const r = compositionReport();
    expect(r.split('\n').length).toBeGreaterThanOrEqual(5);
    expect(r).toMatch(/OK /);
  });
});
