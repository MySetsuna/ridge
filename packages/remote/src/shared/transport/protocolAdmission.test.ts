import { describe, expect, it } from 'vitest';
import {
  admitRemoteMethod,
  canonicalizeMethod,
  isDesktopPrivileged,
  isValidMethodName,
  methodCategory,
  TEAMMATE_REMOTE_REQUIRED,
  validateTeammateHostsBoundary,
} from './protocolAdmission';

describe('protocolAdmission (C55)', () => {
  it('validates names', () => {
    expect(isValidMethodName('write_to_pty')).toBe(true);
    expect(isValidMethodName('')).toBe(false);
    expect(isValidMethodName('bad name')).toBe(false);
  });

  it('canonicalizes aliases', () => {
    expect(canonicalizeMethod('write_pty')).toBe('write_to_pty');
    expect(canonicalizeMethod('search')).toBe('text_search');
  });

  it('blocks remote from desktop privileged', () => {
    expect(admitRemoteMethod('connect_host').ok).toBe(false);
    expect(admitRemoteMethod('list_hitl_pending').ok).toBe(true);
    expect(isDesktopPrivileged('attach_host_session')).toBe(true);
  });

  it('boundary validation', () => {
    const bad = validateTeammateHostsBoundary(['get_teammate_topology', 'connect_host']);
    expect(bad.ok).toBe(false);
    expect(bad.leaks).toContain('connect_host');
    expect(bad.missing.length).toBeGreaterThan(0);

    const good = validateTeammateHostsBoundary([...TEAMMATE_REMOTE_REQUIRED]);
    expect(good.ok).toBe(true);
  });

  it('categorizes methods', () => {
    expect(methodCategory('connect_host')).toBe('desktop_host');
    expect(methodCategory('list_hitl_pending')).toBe('teammate');
    expect(methodCategory('write_to_pty')).toBe('terminal');
  });
});
