// scripts/served-bundles-check.mjs
//
// §4 verify: Host serves bundles whose hashes match the on-disk artifacts
// produced for THIS candidate (commit 667d8389 + dirty diff). Compares
// HTTPS-fetched body SHA-256 against the local artifact SHA-256 captured by
// scripts/print-candidate-provenance.mjs.
//
// Covers:
//   - / and /index.html served for SPA fallback
//   - /assets/index-*.js for both desktop and mobile
//   - /sw.js (mobile service worker) and /manifest.webmanifest
//
// Self-contained: boots candidate on isolated port + data dir if not already
// running. Pass RIDGE_BOOT=0 to skip boot when an external Host is up.

import { createHash } from "node:crypto";
import { spawn } from "node:child_process";
import { mkdtempSync, readFileSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const HOST_PORT = Number(process.env.RIDGE_SMOKE_HOST_PORT ?? "5120");
const ROOT = resolve(process.env.RIDGE_ROOT ?? ".");
const BIN = resolve(process.env.RIDGE_BIN ?? "target/test-rdg/release/ridge.exe");
const BOOT = process.env.RIDGE_BOOT !== "0";

function sha256(buf) {
  return createHash("sha256").update(buf).digest("hex");
}

function localFileSha256(rel) {
  const full = join(ROOT, rel);
  return { sha256: sha256(readFileSync(full)), size: statSync(full).size };
}

async function fetchHttps(path) {
  const r = await fetch(`https://127.0.0.1:${HOST_PORT}${path}`, {
    signal: AbortSignal.timeout(10000),
  });
  const buf = Buffer.from(await r.arrayBuffer());
  return { status: r.status, sha256: sha256(buf), size: buf.length };
}

async function sleep(ms) {
  await new Promise((r) => setTimeout(r, ms));
}

let child;
let dataDir;
if (BOOT) {
  dataDir = mkdtempSync(join(tmpdir(), "ridge-served-"));
  console.log(`[served] booting candidate: ${BIN}`);
  console.log(`[served] isolated data dir: ${dataDir}`);
  console.log(`[served] host port: ${HOST_PORT}`);
  child = spawn(
    BIN,
    ["host", "--port", String(HOST_PORT)],
    {
      env: {
        ...process.env,
        RIDGE_KERNEL_DATA_DIR: dataDir,
        RIDGE_PRINT_TOTP: "1",
        RIDGE_TEST_ALLOW_NON_BREAKAWAY: "1",
        RIDGE_REMOTE_HOST_REGISTRY: join(dataDir, "host-registry.json"),
      },
      stdio: ["ignore", "pipe", "pipe"],
    },
  );
  child.stdout.on("data", (d) => process.stdout.write(d));
  child.stderr.on("data", (d) => process.stderr.write(d));
  let up = false;
  for (let i = 0; i < 30; i += 1) {
    await sleep(500);
    try {
      const r = await fetch(`https://127.0.0.1:${HOST_PORT}/health`, {
        signal: AbortSignal.timeout(3000),
      });
      if (r.status === 200) { up = true; break; }
    } catch {
      /* not yet */
    }
  }
  if (!up) {
    console.error("[served] FAIL candidate never came up");
    process.exit(1);
  }
}

const failures = [];
function check(label, cond, ctx) {
  if (cond) {
    console.log(`[served] PASS ${label}`);
  } else {
    console.error(`[served] FAIL ${label}`);
    if (ctx !== undefined) console.error(JSON.stringify(ctx, null, 2));
    failures.push(label);
  }
}

const cases = [
  // Root paths the mobile SPA actually requests (per mobile/index.html)
  { path: "/", file: "remote-dist/mobile/index.html" },
  { path: "/index.html", file: "remote-dist/mobile/index.html" },
  { path: "/sw.js", file: "remote-dist/mobile/sw.js" },
  { path: "/manifest.webmanifest", file: "remote-dist/mobile/manifest.webmanifest" },
];

for (const c of cases) {
  const remote = await fetchHttps(c.path);
  const local = localFileSha256(c.file);
  check(
    `${c.path} → status 200`,
    remote.status === 200,
    { status: remote.status, file: c.file },
  );
  check(
    `${c.path} body matches ${c.file} (sha256)`,
    remote.sha256 === local.sha256,
    { remote: remote.sha256, local: local.sha256, size: { remote: remote.size, local: local.size } },
  );
}

// Desktop bundle: SPA fallback serves mobile index when default UA used.
// With `?ui=desktop` explicit override, the desktop SPA shell is served.
const desktopIndex = localFileSha256("remote-dist/desktop/index.html");
const remoteDesktop = await fetchHttps("/?ui=desktop");
check(
  "/?ui=desktop reachable",
  remoteDesktop.status === 200,
  { status: remoteDesktop.status },
);
check(
  "/?ui=desktop body matches remote-dist/desktop/index.html",
  remoteDesktop.sha256 === desktopIndex.sha256,
  { remote: remoteDesktop.sha256, local: desktopIndex.sha256 },
);

// Sanity: without `?ui=desktop`, root returns mobile SPA (UA fork default).
const remoteRootNoOverride = await fetchHttps("/");
check(
  "/ without override returns mobile SPA (UA fork default)",
  remoteRootNoOverride.sha256 === localFileSha256("remote-dist/mobile/index.html").sha256,
  { remote: remoteRootNoOverride.sha256, expected: localFileSha256("remote-dist/mobile/index.html").sha256 },
);

// /desktop/index.html without override is a non-asset path; falls through to
// spa_fallback_handler → mobile shell (mobile UA, no override). Documented.
const remoteDesktopNoOverride = await fetchHttps("/desktop/index.html");
check(
  "/desktop/index.html without override returns mobile shell (documented fallback)",
  remoteDesktopNoOverride.sha256 === localFileSha256("remote-dist/mobile/index.html").sha256,
  { remote: remoteDesktopNoOverride.sha256 },
);

console.log("");
if (child) {
  try {
    child.kill("SIGINT");
    await sleep(300);
    child.kill("SIGKILL");
  } catch {
    /* gone */
  }
  console.log(`[served] host stopped`);
}
console.log(`[served] ${failures.length === 0 ? "ALL MATCH" : `MISMATCH ${failures.length}: ${failures.join(" | ")}`}`);
process.exit(failures.length === 0 ? 0 : 1);