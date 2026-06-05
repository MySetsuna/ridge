// scripts/sync-cloud-desktop-app.mjs
//
// Converge the public (cloud) controller page onto the SAME artifact the LAN
// remote server already serves: the desktop SPA `web-remote-dist`.
//
// ridge-cloud bakes a CHECKED-IN copy at `desktop-app/` into its Docker image
// (Dockerfile: `COPY desktop-app ./desktop-app`; DESKTOP_APP_DIR default
// `desktop-app`) and serves it on the tenant subdomain `{device}-{username}.{base}`
// (static_host.rs / router spa_fallback). Without this sync that copy drifts from
// wind's build by hand — a desktop browser over public could load a stale SPA that
// no longer matches the host's protocol/commands (the D9 handshake mitigates, but
// the real fix is: never let them diverge).
//
// This script (1) rebuilds `web-remote-dist` (unless --no-build) and (2) mirrors
// it into `<ridge-cloud>/desktop-app/`. Deploying that copy (commit in ridge-cloud
// + redeploy) stays an explicit ops step — this only removes the manual-copy drift.
//
// ridge-cloud repo path: env `RIDGE_CLOUD_REPO`, else sibling `../ridge-cloud`.
// Missing repo → warn + skip (cloud repo isn't present on every dev machine).

import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, resolve, join } from 'node:path';
import { existsSync } from 'node:fs';
import { rm, mkdir, cp } from 'node:fs/promises';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const isWin = process.platform === 'win32';
const skipBuild = process.argv.includes('--no-build');

const SRC = join(root, 'web-remote-dist');
const cloudRepo =
  process.env.RIDGE_CLOUD_REPO || resolve(root, '..', 'ridge-cloud');
const DST = join(cloudRepo, 'desktop-app');

/** Run `pnpm build:desktop-web` and resolve on success. */
function buildDesktopWeb() {
  return new Promise((res, rej) => {
    const child = spawn(isWin ? 'pnpm.cmd' : 'pnpm', ['build:desktop-web'], {
      cwd: root,
      stdio: 'inherit',
      shell: isWin,
    });
    child.on('exit', (code) =>
      code === 0 ? res() : rej(new Error(`build:desktop-web exited ${code}`)),
    );
  });
}

async function main() {
  if (!skipBuild) {
    console.log('[sync-cloud] building web-remote-dist …');
    await buildDesktopWeb();
  }

  if (!existsSync(join(SRC, 'index.html'))) {
    console.error(
      `[sync-cloud] web-remote-dist not built (no index.html at ${SRC}). ` +
        `Run \`pnpm build:desktop-web\` first, or drop --no-build.`,
    );
    process.exit(1);
  }

  if (!existsSync(cloudRepo)) {
    console.warn(
      `[sync-cloud] ridge-cloud repo not found at ${cloudRepo} — skipping sync. ` +
        `Set RIDGE_CLOUD_REPO to point at it.`,
    );
    return; // graceful: web-remote-dist is built; cloud copy just not updatable here.
  }

  // Mirror: clear the target then copy the fresh build verbatim, so removed
  // assets (old fingerprinted bundles) don't linger and 404s never serve a
  // mismatched shell. desktop-app is git-tracked in ridge-cloud → the diff is
  // reviewable before commit.
  console.log(`[sync-cloud] mirroring\n  ${SRC}\n→ ${DST}`);
  await rm(DST, { recursive: true, force: true });
  await mkdir(DST, { recursive: true });
  await cp(SRC, DST, { recursive: true });

  console.log(
    '[sync-cloud] done. Next (ops): in ridge-cloud, commit desktop-app/ and redeploy ' +
      '(docker build bakes it via `COPY desktop-app`).',
  );
}

main().catch((e) => {
  console.error('[sync-cloud] failed:', e instanceof Error ? e.message : e);
  process.exit(1);
});
