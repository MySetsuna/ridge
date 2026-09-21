---
id: L4-OBS-SCRIPTS-AVD-CATS-MJS-2c8e3f01
level: L4
parent: L3-OBS-SCRIPTS-8c5967fd
title: avd-cats.mjs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: pending
code_targets:
  - scripts/avd-cats.mjs
public_interface:
  - screencap(name) writes to artifacts/release/avd-acceptance/
  - sidebar / scroll / pwa / ime sub-commands via argv
---

# avd-cats.mjs

Per-category gesture driver for AVD. Uses hardcoded original-resolution
coordinates (1344x2992, displayed scale 1.50) because `uiautomator dump`
cannot see WebView buttons. Each subcommand (`sidebar`, `scroll`, `pwa`,
`ime`) takes a `screencap` before and after so PASS/PARTIAL can be
revisited from PNG. Chrome system buttons (3-dot menu) need plain
`input tap` rather than `input swipe`, which is documented inline.
