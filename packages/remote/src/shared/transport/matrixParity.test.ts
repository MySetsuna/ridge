import { describe, expect, it } from 'vitest';
import { REMOTE_ALLOWLIST } from '../cloud/remoteAllowlist';
import {
  capabilityFullyAdmitted,
  reportMatrixParity,
  teammateFromMatrix,
} from './matrixParity';
import { TEAMMATE_REMOTE_REQUIRED } from './protocolAdmission';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

describe('matrixParity (C9 product path)', () => {
  it('shipped allowlist admits teammate controller surface', () => {
    expect(capabilityFullyAdmitted('teammate', REMOTE_ALLOWLIST)).toBe(true);
  });

  it('shipped matrix + allowlist parity when file present', () => {
    const path = resolve(process.cwd(), 'docs/capability-matrix.json');
    let matrix: { capabilities?: Record<string, { methods?: string[] }> };
    try {
      matrix = JSON.parse(readFileSync(path, 'utf8'));
    } catch {
      // skip if cwd not repo root
      return;
    }
    const team = teammateFromMatrix(matrix);
    for (const m of TEAMMATE_REMOTE_REQUIRED) {
      expect(team, m).toContain(m);
    }
    const report = reportMatrixParity(REMOTE_ALLOWLIST, matrix);
    expect(report.ok, report.issues.join('; ')).toBe(true);
  });

  it('detects matrix leak of connect_host', () => {
    const bad = {
      capabilities: {
        teammate: {
          methods: [...TEAMMATE_REMOTE_REQUIRED, 'connect_host'],
        },
      },
    };
    const r = reportMatrixParity(REMOTE_ALLOWLIST, bad);
    expect(r.ok).toBe(false);
    expect(r.teammateLeaks).toContain('connect_host');
  });
});
