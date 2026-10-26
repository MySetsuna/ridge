---
id: L4-OBS-PACKAGE-JSON-7ae45ad1
level: L4
parent: L3-OBS-PACKAGE-JSON-7ae45ad1
title: package.json
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - package.json
  - Cargo.lock
---

# Product release version

package.json supplies the product release version. The Ridge desktop entry in Cargo.lock, desktop Cargo.toml, Tauri bundle version and v-prefixed release tag must match exactly before packaging. Do not rewrite independent crate versions. The release version checker must reject missing or divergent values.
