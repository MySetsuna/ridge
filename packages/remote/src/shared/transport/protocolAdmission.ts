/**
 * CONTRACT-55 / OP-PROTO-DOC+CAP: protocol admission pure layer (TS).
 * Mirrors ridge-core protocol_guard + desktop_surface boundaries.
 */

export const DESKTOP_PRIVILEGED_METHODS = [
  'host_list_snapshot',
  'register_frontend_host',
  'connect_host',
  'disconnect_host',
  'forget_host',
  'attach_host_session',
  'detach_host_session',
  'list_host_sessions',
  'inject_host_output',
  'get_outbound_stats',
  'pump_host_output',
  'bind_mock_outbound_and_list',
  'step_host_reconnect',
  'cancel_host_reconnect',
  'get_foreign_history_tail',
  'append_foreign_history',
  'foreign_history_pull_budget',
  'set_foreign_history_cap',
  'get_live_backpressure',
] as const;

export const METHOD_ALIASES: Record<string, string> = {
  write_pty: 'write_to_pty',
  search: 'text_search',
};

export function isValidMethodName(method: string): boolean {
  const m = method.trim();
  if (!m || m.length > 128) return false;
  return /^[A-Za-z0-9_/$-]+$/.test(m);
}

export function canonicalizeMethod(method: string): string {
  const t = method.trim();
  return METHOD_ALIASES[t] ?? t;
}

export function isDesktopPrivileged(method: string): boolean {
  const c = canonicalizeMethod(method);
  return (DESKTOP_PRIVILEGED_METHODS as readonly string[]).includes(c);
}

export function remoteMayInvoke(method: string, isRemoteController: boolean): boolean {
  if (!isValidMethodName(method)) return false;
  if (isRemoteController && isDesktopPrivileged(method)) return false;
  return true;
}

export type AdmitResult =
  | { ok: true; method: string }
  | { ok: false; method: string; reason: string };

export function admitRemoteMethod(method: string): AdmitResult {
  const c = canonicalizeMethod(method);
  if (!isValidMethodName(c)) {
    return { ok: false, method: c, reason: 'invalid_name' };
  }
  if (!remoteMayInvoke(c, true)) {
    return { ok: false, method: c, reason: 'remote_denied_desktop_privileged' };
  }
  return { ok: true, method: c };
}

export function admitDesktopMethod(method: string): AdmitResult {
  const c = canonicalizeMethod(method);
  if (!isValidMethodName(c)) {
    return { ok: false, method: c, reason: 'invalid_name' };
  }
  return { ok: true, method: c };
}

/** Teammate remote minimum surface (must stay in allowlist). */
export const TEAMMATE_REMOTE_REQUIRED = [
  'get_teammate_topology',
  'list_hitl_pending',
  'list_hitl_audit_remote',
  'resolve_hitl_remote',
  'get_orchestration_health',
  'read_agent_recent_replies',
  'set_teammate_groups',
  'resume_agent_session',
] as const;

export function missingRequired(allowlist: string[], required: readonly string[]): string[] {
  return required.filter((m) => !allowlist.includes(m));
}

export function forbiddenPresent(allowlist: string[], forbidden: readonly string[]): string[] {
  return forbidden.filter((m) => allowlist.includes(m));
}

export function validateTeammateHostsBoundary(allowlist: string[]): {
  ok: boolean;
  missing: string[];
  leaks: string[];
} {
  const missing = missingRequired(allowlist, TEAMMATE_REMOTE_REQUIRED);
  const leaks = forbiddenPresent(allowlist, DESKTOP_PRIVILEGED_METHODS);
  return { ok: missing.length === 0 && leaks.length === 0, missing, leaks };
}

export function methodCategory(
  method: string,
): 'desktop_host' | 'teammate' | 'workspace' | 'terminal' | 'other' {
  const c = canonicalizeMethod(method);
  if (isDesktopPrivileged(c) || c.startsWith('host_') || c.includes('_host_')) return 'desktop_host';
  if (
    c.includes('hitl')
    || c.includes('teammate')
    || c.includes('orchestration')
    || c === 'read_agent_recent_replies'
    || c === 'resume_agent_session'
  ) return 'teammate';
  if (c.includes('workspace')) return 'workspace';
  if (c.includes('pty') || c.includes('terminal') || c.includes('write_to')) return 'terminal';
  return 'other';
}
