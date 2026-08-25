import { describe, expect, it } from 'vitest';
import { WEAK_NET_EVIDENCE_SCOPE, validateWeakNetMetrics } from './weakNetMetrics.mjs';

function validMetrics() {
  return {
    model: 'deterministic-lab',
    evidenceScope: WEAK_NET_EVIDENCE_SCOPE,
    disclaimer: 'Lab evidence only.',
    scenarios: Array.from({ length: 9 }, (_, index) => ({
      family: `scenario-${index}`,
      params: { index },
      observed: { passed: true },
    })),
  };
}

describe('validateWeakNetMetrics', () => {
  it('accepts evidence independently of disclaimer wording', () => {
    const metrics = validMetrics();
    metrics.disclaimer = 'Localized wording may change without changing scope.';
    expect(validateWeakNetMetrics(metrics)).toBe(metrics);
  });

  it('rejects a missing machine-readable evidence scope', () => {
    const metrics = validMetrics();
    delete metrics.evidenceScope;
    expect(() => validateWeakNetMetrics(metrics)).toThrow('evidenceScope');
  });

  it('rejects incomplete scenario coverage', () => {
    const metrics = validMetrics();
    metrics.scenarios.pop();
    expect(() => validateWeakNetMetrics(metrics)).toThrow('below 9');
  });

  it('rejects malformed scenario records', () => {
    const metrics = validMetrics();
    metrics.scenarios[4] = { family: 'broken', params: null, observed: {} };
    expect(() => validateWeakNetMetrics(metrics)).toThrow('malformed');
  });
});
