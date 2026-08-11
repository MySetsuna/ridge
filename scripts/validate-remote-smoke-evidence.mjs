import { existsSync, readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const REQUIRED_SCENARIOS = new Set([
  'baseline',
  'wifi_to_cellular',
  'cellular_to_wifi',
  'background_token_window',
]);
const SENSITIVE_KEY = /(token|jwt|totp|password|secret|authorization|account|hostname|domain)/i;
const RESULTS = new Set(['pass', 'fail', 'blocked']);

function isObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function findSensitiveKey(value, path = '$') {
  if (Array.isArray(value)) {
    for (let i = 0; i < value.length; i += 1) {
      const found = findSensitiveKey(value[i], `${path}[${i}]`);
      if (found) return found;
    }
    return null;
  }
  if (!isObject(value)) return null;
  for (const [key, child] of Object.entries(value)) {
    if (SENSITIVE_KEY.test(key)) return `${path}.${key}`;
    const found = findSensitiveKey(child, `${path}.${key}`);
    if (found) return found;
  }
  return null;
}

export function validateEvidence(value, evidencePath, checkAttachments = true) {
  if (!isObject(value)) return ['root must be an object'];
  const errors = [
    ...validateMetadata(value),
    ...validateDevice(value),
    ...validateScenarios(value),
    ...validateAttachments(value, evidencePath, checkAttachments),
  ];
  if (!RESULTS.has(value.overallResult)) errors.push('overallResult is invalid');
  return errors;
}

function validateMetadata(value) {
  const errors = [];
  const sensitive = findSensitiveKey(value);
  if (sensitive) errors.push(`sensitive field is forbidden: ${sensitive}`);
  if (value.schemaVersion !== 1) errors.push('schemaVersion must be 1');
  if (!Number.isFinite(Date.parse(value.recordedAt))) errors.push('recordedAt must be ISO date-time');
  if (!/^[0-9a-f]{7,40}$/.test(value.gitCommit ?? '')) errors.push('gitCommit must be 7-40 lowercase hex');
  for (const key of ['remoteBuildId', 'cloudDeploymentId']) {
    if (typeof value[key] !== 'string' || value[key].length === 0) errors.push(`${key} is required`);
  }
  if (!['ios-safari', 'android-chrome'].includes(value.platform)) errors.push('platform is invalid');
  return errors;
}

function validateDevice(value) {
  if (!isObject(value.device)) return ['device is required'];
  return ['model', 'osVersion', 'browserVersion']
    .filter((key) => typeof value.device[key] !== 'string' || value.device[key].length === 0)
    .map((key) => `device.${key} is required`);
}

function validateScenarios(value) {
  if (!Array.isArray(value.scenarios)) return ['scenarios must be an array'];
  const errors = [];
  const seen = new Set();
  for (const [index, scenario] of value.scenarios.entries()) {
    errors.push(...validateScenario(scenario, index, seen));
  }
  for (const kind of REQUIRED_SCENARIOS) {
    if (!seen.has(kind)) errors.push(`missing scenario: ${kind}`);
  }
  return errors;
}

function validateScenario(scenario, index, seen) {
  const prefix = `scenarios[${index}]`;
  if (!isObject(scenario)) return [`${prefix} must be an object`];
  const errors = [];
  if (!REQUIRED_SCENARIOS.has(scenario.kind)) errors.push(`${prefix}.kind is invalid`);
  else if (seen.has(scenario.kind)) errors.push(`${prefix}.kind is duplicated`);
  else seen.add(scenario.kind);
  if (!Number.isFinite(Date.parse(scenario.startedAt))) errors.push(`${prefix}.startedAt is invalid`);
  if (!Number.isFinite(Date.parse(scenario.endedAt))) errors.push(`${prefix}.endedAt is invalid`);
  if (!Number.isInteger(scenario.recoveryMs) || scenario.recoveryMs < 0) errors.push(`${prefix}.recoveryMs is invalid`);
  if (!RESULTS.has(scenario.status)) errors.push(`${prefix}.status is invalid`);
  if (typeof scenario.observations !== 'string') errors.push(`${prefix}.observations is required`);
  if (scenario.kind === 'background_token_window' && (!Number.isInteger(scenario.backgroundDurationSeconds) || scenario.backgroundDurationSeconds < 900)) {
    errors.push(`${prefix}.backgroundDurationSeconds must be >= 900`);
  }
  return errors;
}

function validateAttachments(value, evidencePath, checkAttachments) {
  if (!Array.isArray(value.attachments) || value.attachments.length === 0) {
    return ['attachments must be a non-empty array'];
  }
  const errors = [];
  for (const [index, attachment] of value.attachments.entries()) {
    const prefix = `attachments[${index}]`;
    if (!isObject(attachment) || typeof attachment.path !== 'string' || attachment.path.length === 0) {
      errors.push(`${prefix}.path is required`);
      continue;
    }
    if (!['screenshot', 'screen-recording', 'log'].includes(attachment.kind)) errors.push(`${prefix}.kind is invalid`);
    if (checkAttachments && !existsSync(resolve(dirname(evidencePath), attachment.path))) {
      errors.push(`${prefix}.path does not exist: ${attachment.path}`);
    }
  }
  return errors;
}

function selfTest() {
  const base = {
    schemaVersion: 1,
    recordedAt: '2026-07-21T00:00:00.000Z',
    gitCommit: 'abcdef0',
    remoteBuildId: 'remote',
    cloudDeploymentId: 'cloud',
    platform: 'ios-safari',
    device: { model: 'device', osVersion: 'os', browserVersion: 'browser' },
    scenarios: [...REQUIRED_SCENARIOS].map((kind) => ({
      kind,
      startedAt: '2026-07-21T00:00:00.000Z',
      endedAt: '2026-07-21T00:00:01.000Z',
      recoveryMs: 1000,
      ...(kind === 'background_token_window' ? { backgroundDurationSeconds: 960 } : {}),
      status: 'pass',
      observations: '',
    })),
    attachments: [{ kind: 'log', path: 'missing-test-attachment.log' }],
    overallResult: 'pass',
  };
  const missing = structuredClone(base);
  missing.scenarios = missing.scenarios.filter((scenario) => scenario.kind !== 'baseline');
  const sensitive = { ...structuredClone(base), accessToken: 'forbidden' };
  if (!validateEvidence(missing, 'evidence.json', false).some((error) => error.includes('missing scenario'))) {
    throw new Error('self-test failed to reject missing scenario');
  }
  if (!validateEvidence(sensitive, 'evidence.json', false).some((error) => error.includes('sensitive field'))) {
    throw new Error('self-test failed to reject sensitive field');
  }
  if (!validateEvidence(base, 'evidence.json', true).some((error) => error.includes('does not exist'))) {
    throw new Error('self-test failed to reject missing attachment');
  }
  process.stdout.write('remote smoke evidence validator self-test passed\n');
}

const isMain = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  const input = process.argv[2];
  if (input === '--self-test') {
    selfTest();
  } else if (!input) {
    process.stderr.write('usage: node scripts/validate-remote-smoke-evidence.mjs <evidence.json>\n');
    process.exitCode = 2;
  } else {
    const evidencePath = resolve(input);
    let evidence;
    try {
      evidence = JSON.parse(readFileSync(evidencePath, 'utf8'));
    } catch (error) {
      process.stderr.write(`failed to read evidence: ${error instanceof Error ? error.message : String(error)}\n`);
      process.exitCode = 1;
    }
    if (evidence) {
      const errors = validateEvidence(evidence, evidencePath);
      if (errors.length > 0) {
        process.stderr.write(`${errors.join('\n')}\n`);
        process.exitCode = 1;
      } else {
        process.stdout.write(`valid remote smoke evidence: ${evidencePath}\n`);
      }
    }
  }
}
