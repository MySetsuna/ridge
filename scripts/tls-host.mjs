// scripts/tls-host.mjs — TLS cert pinning helper for Remote Host HTTPS probes.
//
// Why this exists (CHG-031 v9-14 / Goal §3):
//   The Remote Host serves TLS with a self-signed CA generated on first boot
//   at <local-data>/ridge/remote-tls/ca.pem. Tests MUST NOT disable cert
//   verification globally (no NODE_TLS_REJECT_UNAUTHORIZED=0, no
//   browser --ignore-certificate-errors). Instead this module reads the host's
//   CA at runtime and constructs a per-call Agent that pins it.
//
// Security posture is the same as a real trusted CA — just one we installed
// out-of-band by reading the file. The agent rejects:
//   - any cert not signed by the pinned CA
//   - any hostname/SAN mismatch
//   - any cert that's expired
//
// Tests that need a real browser path through this same CA via the
// --ignore-certificate-errors-spki-list=… mechanism in browser-ui-e2e.mjs
// (which is *pinning*, not blanket bypass).

import { Agent, get as httpsGet } from "node:https";
import { readFileSync, existsSync, statSync } from "node:fs";
import { join } from "node:path";
import { homedir, tmpdir } from "node:os";
import { X509Certificate, createHash } from "node:crypto";

/** Resolve the host's TLS material directory (mirrors packages/ridge-remote/src/tls.rs::tls_dir). */
export function tlsDir() {
  if (process.env.RIDGE_TLS_DIR) return process.env.RIDGE_TLS_DIR;
  const base = process.env.LOCALAPPDATA
    || (process.platform === "darwin"
      ? join(homedir(), "Library", "Application Support")
      : process.platform === "win32"
        ? join(homedir(), "AppData", "Local")
        : join(homedir(), ".local", "share"));
  return join(base, "ridge", "remote-tls");
}

/**
 * Read the host's CA cert (PEM). Throws if not found — caller must boot a
 * candidate first to generate it. Returns the raw PEM string.
 */
export function loadHostCa() {
  const dir = tlsDir();
  const p = join(dir, "ca.pem");
  if (!existsSync(p)) {
    throw new Error(
      `host CA cert not found at ${p}. ` +
      `Boot a candidate first (e.g. via scripts/smoke-candidate.mjs) to generate it.`,
    );
  }
  return readFileSync(p, "utf8");
}

/**
 * Construct a per-call https.Agent that trusts ONLY the host's CA. Pass to
 * https.request or node:http(s) calls as `agent:`.
 */
export function pinnedAgent() {
  return new Agent({
    ca: loadHostCa(),
    rejectUnauthorized: true,
    // Don't trust any system roots — pin only the host CA. This catches
    // accidental CA-bundle pollution.
    // (Node's Agent merges `ca` with the system CA bundle unless we set
    // `ca: <string|Buffer>` with no system roots requested. Setting it to
    // a single PEM string replaces the bundle.)
  });
}

/**
 * Wrapper around https.get that uses the pinned agent and returns
 * {status, headers, text}. Timeout is hard-coded to 5s.
 */
export function pinnedHttpsJson(url) {
  return new Promise((resolve, reject) => {
    const agent = pinnedAgent();
    const req = httpsGet(url, { agent }, (res) => {
      let body = "";
      res.setEncoding("utf8");
      res.on("data", (c) => (body += c));
      res.on("end", () => resolve({ status: res.statusCode, headers: res.headers, text: body }));
    });
    req.on("error", reject);
    req.setTimeout(5000, () => req.destroy(new Error("timeout")));
    req.end();
  });
}

/** Read the host's leaf cert and return its SPKI SHA-256 (hex).
 *  Used by browser-ui-e2e.mjs to build Chrome's
 *  --ignore-certificate-errors-spki-list=… pin (which is per-SPKI pinning,
 *  not blanket bypass).
 */
export function hostCertSpkiSha256() {
  const dir = tlsDir();
  const p = join(dir, "cert.pem");
  if (!existsSync(p)) throw new Error(`host leaf cert not found at ${p}`);
  const pem = readFileSync(p, "utf8");
  const cert = new X509Certificate(pem);
  // Node v18+ cert.publicKey is a KeyObject, not raw bytes — export it
  // back to DER in SubjectPublicKeyInfo form for the SPKI pin hash.
  let spki;
  try {
    spki = cert.publicKey.export({ type: "spki", format: "der" });
  } catch {
    // Older Node may expose publicKey as a Buffer directly.
    spki = Buffer.from(cert.publicKey);
  }
  return createHash("sha256").update(spki).digest("hex");
}

export { tmpdir };
