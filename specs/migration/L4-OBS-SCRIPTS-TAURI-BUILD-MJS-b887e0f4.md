---
id: L4-OBS-SCRIPTS-TAURI-BUILD-MJS-b887e0f4
level: L4
parent: L3-OBS-SCRIPTS-8c5967fd
title: tauri-build.mjs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - scripts/tauri-build.mjs
test_targets:
  - scripts/app-csp.test.mjs
  - scripts/build-ridge-mcp-sidecar.test.mjs
  - scripts/build-ridge.test.mjs
  - scripts/cdp-port.test.mjs
  - scripts/cdp-pty-state.test.mjs
  - scripts/cdp-reap-test.mjs
  - scripts/check-release-version.test.mjs
  - scripts/cloud-protocol-ssot.test.ts
  - scripts/lib/cdpModuleUrl.test.mjs
  - scripts/lib/cdpTarget.test.mjs
  - scripts/lib/remoteArtifactBundle.test.mjs
  - scripts/lib/toolPath.test.mjs
  - scripts/lib/weakNetMetrics.test.mjs
  - scripts/quality-helpers.test.mjs
  - scripts/rdg-remote-e2e.test.ts
  - scripts/remote-createws-test.mjs
  - scripts/remote-runtime-last-error-attribution.test.ts
  - scripts/start-vite-dev.test.mjs
  - scripts/sync-generated-csp.test.mjs
  - scripts/tauri-dev-cdp-env.test.mjs
  - scripts/validate-remote-smoke-evidence.test.mjs
public_interface:
  - export function buildPlan(envSource = process.env, platform =
    process.platform, spawnSyncImpl = spawnSync)
  - export function hasBin(name, spawnSyncImpl = spawnSync, platform =
    process.platform)
  - export function main({ envSource = process.env, platform = process.platform,
    spawnImpl = spawn, spawnSyncImpl = spawnSync, io = console, now = Date.now }
    = {})
verified_by:
  - TEST-OBS-SCRIPTS-APP-CSP-TEST-MJS-8c76d58b
  - TEST-OBS-SCRIPTS-BUILD-RIDGE-MCP-SIDECAR-TEST-MJS-a86f29b5
  - TEST-OBS-SCRIPTS-BUILD-RIDGE-TEST-MJS-86e854f4
  - TEST-OBS-SCRIPTS-CDP-PORT-TEST-MJS-857ddaea
  - TEST-OBS-SCRIPTS-CDP-PTY-STATE-TEST-MJS-462b9d95
  - TEST-OBS-SCRIPTS-CDP-REAP-TEST-MJS-cff56471
  - TEST-OBS-SCRIPTS-CHECK-RELEASE-VERSION-TEST-MJS-ff79f177
  - TEST-OBS-SCRIPTS-CLOUD-PROTOCOL-SSOT-TEST-TS-00c3d7f0
  - TEST-OBS-SCRIPTS-LIB-CDPMODULEURL-TEST-MJS-2cfe59e9
  - TEST-OBS-SCRIPTS-LIB-CDPTARGET-TEST-MJS-1120ba97
  - TEST-OBS-SCRIPTS-LIB-REMOTEARTIFACTBUNDLE-TEST-MJS-75c40031
  - TEST-OBS-SCRIPTS-LIB-TOOLPATH-TEST-MJS-e11cd513
  - TEST-OBS-SCRIPTS-LIB-WEAKNETMETRICS-TEST-MJS-6b840696
  - TEST-OBS-SCRIPTS-QUALITY-HELPERS-TEST-MJS-5d2356c4
  - TEST-OBS-SCRIPTS-RDG-REMOTE-E2E-TEST-TS-2c9bc9e5
  - TEST-OBS-SCRIPTS-REMOTE-CREATEWS-TEST-MJS-e9b94290
  - TEST-OBS-SCRIPTS-REMOTE-RUNTIME-LAST-ERROR-ATTRIBUTION-TEST-TS-812bdc3e
  - TEST-OBS-SCRIPTS-START-VITE-DEV-TEST-MJS-0823d5ca
  - TEST-OBS-SCRIPTS-SYNC-GENERATED-CSP-TEST-MJS-d2aa62c5
  - TEST-OBS-SCRIPTS-TAURI-DEV-CDP-ENV-TEST-MJS-f0b53c0d
  - TEST-OBS-SCRIPTS-VALIDATE-REMOTE-SMOKE-EVIDENCE-TEST-MJS-a68aae34
---

# tauri-build.mjs

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
