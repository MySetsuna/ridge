// src/lib/utils/linkTrust.ts
//
// Session-scoped allowlist for opening external URLs from MarkdownPreview.
// First time a host is touched in a session we ask the user to confirm; once
// confirmed the host is added to an in-memory Set so the second click of the
// same docs page opens immediately.
//
// Trust is now scoped per basePath (the directory of the .md file being
// previewed). A trust granted inside ~/untrusted-repo/README.md does NOT
// carry over to ~/my-project/README.md — matching VS Code's workspace-folder
// trust model. `basePath = ''` is treated as a global fallback for callers
// that don't have a basePath (rare).
//
// Why session-scoped: persisting trust to disk would silently widen the
// attack surface across runs — a markdown file inside a freshly-cloned
// untrusted repo could quietly point at a host you OK'd six months ago.

/**
 * Schemes we never prompt for. `mailto:` / `tel:` already trigger an OS app
 * picker; those flows have their own confirmation. `file:` is handled
 * separately by the markdown anchor pipeline (treated as a workspace path,
 * not as an external link). Anything else (http/https/ftp/custom) goes
 * through the prompt.
 */
const SKIP_PROMPT_SCHEMES = new Set(['mailto:', 'tel:']);

/** Map from normalized basePath → set of trusted host keys. */
const trustedByBase = new Map<string, Set<string>>();

function normalizeBase(basePath: string | undefined): string {
  return (basePath ?? '').replace(/[\\/]+$/, '').toLowerCase();
}

function getOrCreateSet(basePath: string | undefined): Set<string> {
  const key = normalizeBase(basePath);
  let set = trustedByBase.get(key);
  if (!set) {
    set = new Set<string>();
    trustedByBase.set(key, set);
  }
  return set;
}

/**
 * Return the host portion of `url` for trust-set keying. We canonicalise to
 * lowercase and strip the leading `www.` so that "github.com" and
 * "www.github.com" share an entry — distinct subdomains (`api.github.com`)
 * still get their own prompt because that's where the security boundary
 * actually lives. Returns `null` for URLs without a host (mailto, tel, …)
 * or anything `URL` rejects.
 */
export function hostKeyFromUrl(url: string): string | null {
  try {
    const u = new URL(url);
    if (!u.host) return null;
    return u.host.toLowerCase().replace(/^www\./, '');
  } catch {
    return null;
  }
}

/**
 * Is `url`'s host already trusted for this basePath?
 * Schemes in `SKIP_PROMPT_SCHEMES` always count as trusted.
 */
export function isTrustedUrl(url: string, basePath?: string): boolean {
  try {
    const u = new URL(url);
    if (SKIP_PROMPT_SCHEMES.has(u.protocol)) return true;
  } catch {
    return false;
  }
  const key = hostKeyFromUrl(url);
  if (!key) return false;
  return getOrCreateSet(basePath).has(key);
}

/** Add `url`'s host to the trusted set for `basePath`. No-op for hostless URLs. */
export function trustHostFromUrl(url: string, basePath?: string): void {
  const key = hostKeyFromUrl(url);
  if (key) getOrCreateSet(basePath).add(key);
}

/** Test-only — clears all trust sets between cases. */
export function _resetTrustedHosts_forTests(): void {
  trustedByBase.clear();
}
