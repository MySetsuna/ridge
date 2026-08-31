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

# package.json

The root package version is the release source of truth. Cargo.lock records the matching Ridge package version, and the release version contract requires both values to equal the pushed v-prefixed tag.
