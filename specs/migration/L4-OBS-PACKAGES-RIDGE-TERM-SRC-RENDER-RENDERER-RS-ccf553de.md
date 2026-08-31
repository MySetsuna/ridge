---
id: L4-OBS-PACKAGES-RIDGE-TERM-SRC-RENDER-RENDERER-RS-ccf553de
level: L4
parent: L3-OBS-PACKAGES-RIDGE-TERM-SRC-RENDER-0f92aefb
title: renderer.rs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/ridge-term/src/render/renderer.rs
test_targets:
  - src/lib/terminal/rendererBackend.test.ts
verified_by:
  - TEST-OBS-SRC-LIB-TERMINAL-RENDERERBACKEND-TEST-TS-3fecf7c5
---

# renderer.rs

The native hyperlink pass draws only structurally safe HTTP(S) OSC-8 spans; unsupported schemes and malformed targets retain text but no hyperlink underline.
