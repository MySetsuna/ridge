---
id: L4-OBS-CLAUDE-SETTINGS-LOCAL-JSON-fca16cae
level: L4
parent: L3-OBS-CLAUDE-e026f56c
title: settings.local.json
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - .claude/settings.local.json
  - .stcignore
---

# settings.local.json

Claude settings.local.json is machine-local permission state rather than product source. SpecTree excludes it through .stcignore so local permission churn cannot block unrelated semantic changes; the one-time transition remains explicitly authorized and traceable.
