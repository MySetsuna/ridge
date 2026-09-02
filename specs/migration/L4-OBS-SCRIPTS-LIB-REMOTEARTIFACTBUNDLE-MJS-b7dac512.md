---
id: L4-OBS-SCRIPTS-LIB-REMOTEARTIFACTBUNDLE-MJS-b7dac512
level: L4
parent: L3-OBS-SCRIPTS-LIB-68e4790e
title: remoteArtifactBundle.mjs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - scripts/lib/remoteArtifactBundle.mjs
test_targets:
  - scripts/lib/cdpModuleUrl.test.mjs
  - scripts/lib/cdpTarget.test.mjs
  - scripts/lib/remoteArtifactBundle.test.mjs
  - scripts/lib/toolPath.test.mjs
  - scripts/lib/weakNetMetrics.test.mjs
public_interface:
  - export function buildManifest({ version, gitSha, builtAt })
  - export function collectFiles(dir, prefix)
  - export function packBundle(manifest, files)
  - export function readArtifactMetadata(dir)
  - export function resolveConfig(env, argv)
  - export function writeArtifactMetadata(dir, manifest)
verified_by:
  - TEST-OBS-SCRIPTS-LIB-CDPMODULEURL-TEST-MJS-2cfe59e9
  - TEST-OBS-SCRIPTS-LIB-CDPTARGET-TEST-MJS-1120ba97
  - TEST-OBS-SCRIPTS-LIB-REMOTEARTIFACTBUNDLE-TEST-MJS-75c40031
  - TEST-OBS-SCRIPTS-LIB-TOOLPATH-TEST-MJS-e11cd513
  - TEST-OBS-SCRIPTS-LIB-WEAKNETMETRICS-TEST-MJS-6b840696
---

# remoteArtifactBundle.mjs

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
