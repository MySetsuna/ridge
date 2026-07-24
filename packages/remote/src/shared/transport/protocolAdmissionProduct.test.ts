/**
 * Product-path parity: TS protocolAdmission mirrors ridge-core protocol_guard
 * catalog used by remote_host_impl admit_remote_method.
 */
import { describe, expect, it } from 'vitest';
import {
  admitRemoteMethod,
  DESKTOP_PRIVILEGED_METHODS,
  TEAMMATE_REMOTE_REQUIRED,
  validateTeammateHostsBoundary,
  methodCategory,
} from './protocolAdmission';

describe('protocolAdmission product path (C55)', () => {
  it('denies every desktop privileged catalog entry', () => {
    for (const m of DESKTOP_PRIVILEGED_METHODS) {
      const r = admitRemoteMethod(m);
      expect(r.ok, m).toBe(false);
      expect(methodCategory(m)).toBe('desktop_host');
    }
  });

  it('allows teammate remote required surface', () => {
    for (const m of TEAMMATE_REMOTE_REQUIRED) {
      expect(admitRemoteMethod(m).ok, m).toBe(true);
    }
  });

  it('boundary rejects allowlist that leaks connect_host', () => {
    const r = validateTeammateHostsBoundary([
      ...TEAMMATE_REMOTE_REQUIRED,
      'connect_host',
    ]);
    expect(r.ok).toBe(false);
    expect(r.leaks).toContain('connect_host');
  });
});
