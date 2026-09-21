---
id: L4-OBS-SCRIPTS-AVD-ADB-PS1-9d2b6c41
level: L4
parent: L3-OBS-SCRIPTS-8c5967fd
title: avd-adb.ps1
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: pending
code_targets:
  - scripts/avd-adb.ps1
public_interface:
  - scripts/avd-adb.ps1 forwards adb arguments via $args
---

# avd-adb.ps1

PowerShell wrapper that invokes `adb.exe` with positional args. Needed for
AVD acceptance flows because Git Bash MSYS layer rewrites `/data/...`
arguments into `C:/DevKit/Git/data/...` on `adb shell cat`. PowerShell
does not apply that mangling, so paths survive intact.
