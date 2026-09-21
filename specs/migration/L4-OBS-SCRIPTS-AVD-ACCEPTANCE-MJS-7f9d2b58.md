---
id: L4-OBS-SCRIPTS-AVD-ACCEPTANCE-MJS-7f9d2b58
level: L4
parent: L3-OBS-SCRIPTS-8c5967fd
title: avd-acceptance.mjs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: pending
code_targets:
  - scripts/avd-acceptance.mjs
public_interface:
  - boots test-rdg host, captures TOTP, opens chrome at SPA URL
  - saves host-info.json + screenshots to artifacts/release/avd-acceptance/
---

# avd-acceptance.mjs

Initial AVD acceptance driver. Boots a fresh test-rdg host on port 5120
with isolated data dir, captures TOTP, opens Chrome on the SPA URL,
attempts `input text` + `input keyevent 66` (ENTER) for auth. Kept as
historical reference — the more reliable end-to-end driver is
`avd-auth-and-probe.mjs` (uses `input swipe` dwell to register Svelte 5
buttons).
