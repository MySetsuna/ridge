import { describe, expect, it } from 'vitest';
import { validateEvidence } from './validate-remote-smoke-evidence.mjs';

const scenarioKinds = [
  'baseline',
  'wifi_to_cellular',
  'cellular_to_wifi',
  'background_token_window',
];

function validEvidence() {
  return {
    schemaVersion: 1,
    recordedAt: '2026-07-21T00:00:00.000Z',
    gitCommit: 'abcdef0',
    remoteBuildId: 'remote',
    cloudDeploymentId: 'cloud',
    platform: 'android-chrome',
    device: { model: 'Pixel', osVersion: 'Android', browserVersion: 'Chrome' },
    scenarios: scenarioKinds.map((kind) => ({
      kind,
      startedAt: '2026-07-21T00:00:00.000Z',
      endedAt: '2026-07-21T00:00:01.000Z',
      recoveryMs: 0,
      ...(kind === 'background_token_window' ? { backgroundDurationSeconds: 900 } : {}),
      status: 'pass',
      observations: 'ok',
    })),
    attachments: [{ kind: 'log', path: 'evidence.log' }],
    overallResult: 'pass',
  };
}

describe('remote smoke evidence validator', () => {
  it('accepts a complete evidence matrix when attachment lookup is disabled', () => {
    expect(validateEvidence(validEvidence(), 'evidence.json', false)).toEqual([]);
  });

  it('rejects malformed metadata, duplicate/missing scenarios, and unsafe fields', () => {
    const value = validEvidence();
    value.accessToken = 'must-not-leak';
    value.schemaVersion = 2;
    value.recordedAt = 'not-a-date';
    value.gitCommit = 'NOT-HEX';
    value.platform = 'desktop';
    value.device.browserVersion = '';
    value.scenarios = [
      { ...value.scenarios[0], kind: 'baseline', recoveryMs: -1, status: 'unknown' },
      { ...value.scenarios[0], kind: 'baseline', observations: 1 },
      { kind: 'background_token_window', startedAt: 'bad', endedAt: 'bad', recoveryMs: 1, status: 'pass', observations: '', backgroundDurationSeconds: 1 },
      null,
    ];
    value.attachments = [{ kind: 'video', path: '' }];
    value.overallResult = 'unknown';

    const errors = validateEvidence(value, 'evidence.json', false);
    expect(errors).toEqual(expect.arrayContaining([
      'sensitive field is forbidden: $.accessToken',
      'schemaVersion must be 1',
      'recordedAt must be ISO date-time',
      'gitCommit must be 7-40 lowercase hex',
      'platform is invalid',
      'device.browserVersion is required',
      'scenarios[1].kind is duplicated',
      'scenarios[2].backgroundDurationSeconds must be >= 900',
      'scenarios[3] must be an object',
      'attachments[0].path is required',
      'overallResult is invalid',
    ]));
    expect(errors.some((error) => error.startsWith('missing scenario:'))).toBe(true);
  });

  it('checks attachment existence only when requested', () => {
    const value = validEvidence();
    expect(validateEvidence(value, 'C:/missing/evidence.json', true)).toContain(
      'attachments[0].path does not exist: evidence.log',
    );
    expect(validateEvidence(null, 'evidence.json', false)).toEqual(['root must be an object']);
  });
});
