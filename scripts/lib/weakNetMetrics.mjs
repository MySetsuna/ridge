export const WEAK_NET_EVIDENCE_SCOPE = 'deterministic-lab-only';

export function validateWeakNetMetrics(metrics) {
  if (!metrics || typeof metrics !== 'object') {
    throw new Error('metrics must be an object');
  }
  if (metrics.model !== 'deterministic-lab') {
    throw new Error('model must be deterministic-lab');
  }
  if (metrics.evidenceScope !== WEAK_NET_EVIDENCE_SCOPE) {
    throw new Error(`evidenceScope must be ${WEAK_NET_EVIDENCE_SCOPE}`);
  }
  if (typeof metrics.disclaimer !== 'string' || metrics.disclaimer.trim().length === 0) {
    throw new Error('disclaimer must be a non-empty string');
  }
  if (!Array.isArray(metrics.scenarios) || metrics.scenarios.length < 9) {
    throw new Error(`scenario count ${metrics.scenarios?.length ?? 0} is below 9`);
  }
  for (const scenario of metrics.scenarios) {
    if (
      !scenario?.family ||
      !scenario.params ||
      typeof scenario.params !== 'object' ||
      !scenario.observed ||
      typeof scenario.observed !== 'object'
    ) {
      throw new Error(`scenario is malformed: ${JSON.stringify(scenario)}`);
    }
  }
  return metrics;
}
