import { describe, expect, it } from 'vitest';
import {
  decideRemoteInvoke,
  filterAdmittedMethods,
  withRemoteAdmission,
} from './remoteInvokeAdmit';

describe('remoteInvokeAdmit (C10 product path)', () => {
  it('denies connect_host and aliases write_pty', () => {
    const d = decideRemoteInvoke('connect_host');
    expect(d.allow).toBe(false);
    const w = decideRemoteInvoke('write_pty');
    expect(w.allow).toBe(true);
    if (w.allow) expect(w.method).toBe('write_to_pty');
  });

  it('allows teammate list_hitl_pending', () => {
    const d = decideRemoteInvoke('list_hitl_pending');
    expect(d.allow).toBe(true);
    expect(d.category).toBe('teammate');
  });

  it('filter batch separates allow/deny', () => {
    const r = filterAdmittedMethods([
      'list_hitl_pending',
      'connect_host',
      'get_orchestration_health',
    ]);
    expect(r.allowed).toContain('list_hitl_pending');
    expect(r.denied.some((x) => x.method === 'connect_host')).toBe(true);
  });

  it('withRemoteAdmission wraps invoke', async () => {
    const calls: string[] = [];
    const inv = withRemoteAdmission(async (m) => {
      calls.push(m);
      return 'ok' as const;
    });
    await expect(inv('connect_host')).rejects.toThrow(/denied/);
    await expect(inv('list_hitl_pending')).resolves.toBe('ok');
    expect(calls).toEqual(['list_hitl_pending']);
  });
});
