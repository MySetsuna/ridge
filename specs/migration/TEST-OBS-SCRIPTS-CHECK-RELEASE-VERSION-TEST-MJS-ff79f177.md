---
id: TEST-OBS-SCRIPTS-CHECK-RELEASE-VERSION-TEST-MJS-ff79f177
level: TEST
parent: L3-OBS-SCRIPTS-8c5967fd
title: check-release-version.test.mjs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
test_targets:
  - scripts/check-release-version.test.mjs
---

# Release version regression tests

Exercise real manifest parsing and rejection of missing or inconsistent versions across the four release inputs. Verify exact v-prefixed tag matching and rejection of a different release tag; no tag remains valid for local pre-release checks.
