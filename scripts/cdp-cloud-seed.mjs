#!/usr/bin/env node
// DEV-ONLY: seed the local ridge-cloud (:5050) with a premium user + bound device,
// then print the userToken (controller) + deviceToken (host) for the B1 e2e harness.
//
// Fresh unique username/email/device per run (timestamp suffix) → idempotent, no
// collisions across runs. Premium is set directly in the docker postgres (ridge-pg).
//
// Usage: node scripts/cdp-cloud-seed.mjs   (requires :5050 up + docker ridge-pg up)
// Output: a JSON blob on the last line with { userToken, deviceToken, username, device }.

import { execFileSync } from 'node:child_process';

const BASE = process.env.RIDGE_CLOUD_LOCAL ?? 'http://localhost:5050/api/v1';
const PASS = 'RidgeLocalDev_123';
const sfx = Date.now().toString(36).slice(-6); // lowercase alnum
const username = `a${sfx}`.slice(0, 20); // ^[a-z0-9]{3,20}$
const email = `${username}@example.com`;
const device = `dev${sfx}`.slice(0, 30); // ^[a-z0-9]([a-z0-9-]*[a-z0-9])?$

async function post(path, body, token) {
  const res = await fetch(`${BASE}${path}`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    body: JSON.stringify(body ?? {}),
  });
  let j;
  try {
    j = await res.json();
  } catch {
    j = { ok: false, error: { code: 'BAD_RESPONSE', message: `HTTP ${res.status}` } };
  }
  return { status: res.status, j };
}

function die(msg, detail) {
  console.error(`[seed] FAIL: ${msg}`, detail ? JSON.stringify(detail) : '');
  process.exit(1);
}

const reg = await post('/auth/register', { email, password: PASS });
if (!reg.j.ok) die('register', reg.j);
let userToken = reg.j.data.token;
console.error(`[seed] registered ${email}`);

const su = await post('/auth/set-username', { username }, userToken);
if (!su.j.ok) die('set-username', su.j);
console.error(`[seed] username = ${username}`);

// premium via the configured Postgres (DB-authoritative; WS controller gate reads the DB).
// Prefer a host URL for native/local Postgres. Docker remains the default fallback.
const dbUrl = process.env.RIDGE_CLOUD_DB_URL;
const dbPassword = process.env.RIDGE_CLOUD_DB_PASSWORD;
const psql = process.env.RIDGE_CLOUD_PSQL ?? 'psql';
// The checked-in local Cloud E2E service points at this database. Keep an
// override for other dev stacks instead of silently promoting a different DB.
const dbName = process.env.RIDGE_CLOUD_DB_NAME ?? 'ridge_cloud_e2e';
const escapedUsername = username.replaceAll("'", "''");
const promoteSql = `UPDATE users SET plan='premium', premium_expires_at=NULL WHERE username='${escapedUsername}' RETURNING username;`;
let promoteOutput = '';
if (dbUrl) {
  promoteOutput = execFileSync(
    psql,
    ['--dbname', dbUrl, '-v', 'ON_ERROR_STOP=1', '-c', promoteSql],
    {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'inherit'],
      env: { ...process.env, ...(dbPassword ? { PGPASSWORD: dbPassword } : {}) },
    },
  );
} else {
  promoteOutput = execFileSync(
    'docker',
    [
      'exec',
      ...(dbPassword ? ['-e', `PGPASSWORD=${dbPassword}`] : []),
      'ridge-pg',
      'psql',
      '-U',
      'postgres',
      '-d',
      dbName,
      '-c',
      promoteSql,
    ],
    { encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'] },
  );
}
if (!promoteOutput.split(/\r?\n/).some((line) => line.trim() === username)) {
  die('premium grant updated no matching user', { dbName, username, output: promoteOutput.trim() });
}
console.error('[seed] premium granted');

// fresh login → token now carries username + premium plan.
const lo = await post('/auth/login', { email, password: PASS });
if (!lo.j.ok) die('login', lo.j);
userToken = lo.j.data.token;

// device/bind → creates device row + returns device JWT (host token), no premium needed.
const bind = await post('/device/bind', { device_name: device }, userToken);
if (!bind.j.ok) die('device/bind', bind.j);
const deviceToken = bind.j.data.token;
console.error(`[seed] device bound = ${device}`);

console.log(JSON.stringify({ userToken, deviceToken, username, device }));
