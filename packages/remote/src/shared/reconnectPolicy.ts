/**
 * R17-RECONN: pure reconnect backoff — parity with Rust `reconnect_policy::backoff_ms`.
 * Shipped call sites: controllerCloudProvider / ridgeCloudProvider scheduleReconnect.
 */

export const RECONNECT_BASE_MS = 1_000;
export const RECONNECT_MAX_MS = 15_000;

/** min(maxMs, baseMs * 2^attempt), attempt capped at 16 to avoid overflow. */
export function backoffMs(
  attempt: number,
  baseMs: number = RECONNECT_BASE_MS,
  maxMs: number = RECONNECT_MAX_MS,
): number {
  const base = Math.max(1, baseMs);
  const max = Math.max(base, maxMs);
  const shift = Math.min(Math.max(0, Math.trunc(attempt)), 16);
  const exp = base * 2 ** shift;
  return Math.min(exp, max);
}

export function shouldRetry(attempt: number, maxAttempts: number): boolean {
  return attempt < maxAttempts;
}
