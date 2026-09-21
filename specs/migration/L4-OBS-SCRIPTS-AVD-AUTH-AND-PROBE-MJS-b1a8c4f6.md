---
id: L4-OBS-SCRIPTS-AVD-AUTH-AND-PROBE-MJS-b1a8c4f6
level: L4
parent: L3-OBS-SCRIPTS-8c5967fd
title: avd-auth-and-probe.mjs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: pending
code_targets:
  - scripts/avd-auth-and-probe.mjs
public_interface:
  - boots isolated ridge host on port 5120
  - captures TOTP from stderr
  - drives chrome via adb swipeTap + screencap
---

# avd-auth-and-probe.mjs

Driver for §12.6 device-category acceptance via AVD. Boots a fresh
test-rdg host (`target/test-rdg/release/ridge.exe host --port 5120`)
with isolated `RIDGE_KERNEL_DATA_DIR` so it does not collide with the
installed ridge (PIDs 17384/17584 untouched). Captures the printed
TOTP, force-stops Chrome on the AVD, restarts it at the host URL so
the SPKI-pinned chrome-command-line takes effect, then drives
the SPA auth form via `adb shell input swipe X Y X Y MS` (the plain
`input tap` does not register Svelte 5 delegated click handlers on
this AVD). Holds the host alive for 5 minutes so follow-up
adb-input commands can exercise subsequent categories.
