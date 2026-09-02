---
id: L4-OBS-PACKAGES-RIDGE-REMOTE-SRC-AUTH-RS-14780bb8
level: L4
parent: L3-OBS-PACKAGES-RIDGE-REMOTE-SRC-e755ff2a
title: packages/ridge-remote/src/auth.rs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/ridge-remote/src/auth.rs
test_targets:
  - packages/ridge-remote/src/auth.rs
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-AUTH-TEST-TS-8907fcc2
---

# packages/ridge-remote/src/auth.rs

LAN session records remain device- and source-IP-bound and expire after 24 hours, so active devices verify at most once per day without weakening strict token validation.
