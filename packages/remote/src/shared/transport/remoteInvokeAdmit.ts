/**
 * AC4-C10 product path: admit remote controller invokes **before** host bridge.
 * Cloud/LAN controller paths call this so desktop-privileged methods never
 * reach Tauri even if allowlist is misconfigured.
 */

import {
  admitRemoteMethod,
  isDesktopPrivileged,
  methodCategory,
  type AdmitResult,
} from './protocolAdmission';

export type RemoteInvokeDecision =
  | { allow: true; method: string; category: ReturnType<typeof methodCategory> }
  | { allow: false; method: string; reason: string; category: ReturnType<typeof methodCategory> };

export function decideRemoteInvoke(rawMethod: string): RemoteInvokeDecision {
  const admitted: AdmitResult = admitRemoteMethod(rawMethod);
  const method = admitted.ok ? admitted.method : rawMethod.trim();
  const category = methodCategory(method);
  if (!admitted.ok) {
    return {
      allow: false,
      method,
      reason: admitted.reason,
      category,
    };
  }
  // Defense in depth: desktop privileged always deny for remote controller role
  if (isDesktopPrivileged(method)) {
    return {
      allow: false,
      method,
      reason: 'desktop_privileged',
      category,
    };
  }
  return { allow: true, method, category };
}

/** Batch preflight for multi-method UI panels. */
export function filterAdmittedMethods(methods: string[]): {
  allowed: string[];
  denied: { method: string; reason: string }[];
} {
  const allowed: string[] = [];
  const denied: { method: string; reason: string }[] = [];
  for (const m of methods) {
    const d = decideRemoteInvoke(m);
    if (d.allow) allowed.push(d.method);
    else denied.push({ method: d.method, reason: d.reason });
  }
  return { allowed, denied };
}

/**
 * Wrap an invoke function with admission (product adapter for cloudHostBridge).
 */
export function withRemoteAdmission<T>(
  invoke: (method: string, params?: Record<string, unknown>) => Promise<T>,
): (method: string, params?: Record<string, unknown>) => Promise<T> {
  return async (method, params) => {
    const d = decideRemoteInvoke(method);
    if (!d.allow) {
      throw new Error(`remote invoke denied: ${d.method} (${d.reason})`);
    }
    return invoke(d.method, params);
  };
}
