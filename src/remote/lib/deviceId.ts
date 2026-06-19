// A stable per-browser device identifier for the LAN remote client.
//
// Sent on /verify (token issuance) and /ws (connection) so the server can pin
// the session token to this device IN ADDITION to its source IP (audit H5/L-3).
// Without it the binding degrades to IP-only, which lets a token replayed from a
// second device behind the SAME NAT egress IP still validate. The id is an
// opaque random UUID — it carries no PII and never leaves this origin's
// localStorage except as the `device` auth parameter.

const DEVICE_KEY = 'ridge_remote_device';

/** Get (or lazily create + persist) this browser's stable device id. Returns an
 *  empty string when storage is unavailable (e.g. private mode); the server then
 *  falls back to the IP pin, so auth still works — just without the device
 *  factor. The same value MUST be sent at token issuance (/verify) and on every
 *  /ws (re)connect, otherwise the server's device pin would reject the session. */
export function getRemoteDeviceId(): string {
  try {
    let id = localStorage.getItem(DEVICE_KEY);
    if (!id) {
      id =
        typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function'
          ? crypto.randomUUID()
          : `dev-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
      localStorage.setItem(DEVICE_KEY, id);
    }
    return id;
  } catch {
    return '';
  }
}
