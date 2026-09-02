---
id: L3-OBS-SCRIPTS-LIB-68e4790e
level: L3
parent: L2-OBS-SCRIPTS-8c5967fd
title: scripts/lib module
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - scripts/lib/cdpModuleUrl.mjs
  - scripts/lib/cdpTarget.mjs
  - scripts/lib/remoteArtifactBundle.mjs
  - scripts/lib/toolPath.mjs
  - scripts/lib/weakNetMetrics.mjs
public_interface:
  - export function buildManifest({ version, gitSha, builtAt })
  - export function cargoTool(name)
  - export function collectFiles(dir, prefix)
  - export function gitTool()
  - export function isRidgeCdpTarget(target, expectedOrigin)
  - export function packBundle(manifest, files)
  - export function pnpmInvocation()
  - export function readArtifactMetadata(dir)
  - export function resolveCdpModuleUrl(devUrl, specifier)
  - export function resolveConfig(env, argv)
  - export function systemTool(name)
  - export function validateWeakNetMetrics(metrics)
  - export function writeArtifactMetadata(dir, manifest)
---

# scripts/lib module

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
