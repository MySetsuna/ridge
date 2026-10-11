const TOKEN_KEY = 'ridge_remote_token';

export function readRemoteToken(): string | null {
  try {
    const token = sessionStorage.getItem(TOKEN_KEY);
    if (token) return token;
  } catch { /* use persistent storage */ }
  try {
    return localStorage.getItem(TOKEN_KEY);
  } catch { return null; }
}

export function writeRemoteToken(token: string): void {
  try {
    localStorage.setItem(TOKEN_KEY, token);
    try { sessionStorage.removeItem(TOKEN_KEY); } catch { /* unavailable */ }
    return;
  } catch { /* use tab storage */ }
  try { sessionStorage.setItem(TOKEN_KEY, token); } catch { /* unavailable */ }
}

export function clearRemoteToken(): void {
  try { localStorage.removeItem(TOKEN_KEY); } catch { /* unavailable */ }
  try { sessionStorage.removeItem(TOKEN_KEY); } catch { /* unavailable */ }
}

/** Unavailable checks never invalidate credentials. The server owns expiry. */
export async function remoteSessionValidity(token: string, signal: AbortSignal): Promise<boolean | null> {
  try {
    const response = await fetch(`/session?token=${encodeURIComponent(token)}`, { signal, cache: 'no-store' });
    if (!response.ok) return null;
    const body = await response.json();
    return typeof body?.valid === 'boolean' ? body.valid : null;
  } catch { return null; }
}
