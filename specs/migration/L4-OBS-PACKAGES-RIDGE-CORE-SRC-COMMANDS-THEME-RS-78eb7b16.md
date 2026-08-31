---
id: L4-OBS-PACKAGES-RIDGE-CORE-SRC-COMMANDS-THEME-RS-78eb7b16
level: L4
parent: L3-OBS-PACKAGES-RIDGE-CORE-SRC-COMMANDS-a40a918d
title: theme.rs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/ridge-core/src/commands/theme.rs
test_targets:
  - packages/remote/src/shared/terminal/themeBridge.test.ts
  - src/lib/components/customTheme.test.ts
  - src/lib/monaco/ridgeTheme.test.ts
  - src/lib/stores/themes.slug.test.ts
  - src/lib/stores/themes.test.ts
  - src/remote/lib/theme.test.ts
  - tests/e2e-shell/theme-injection.spec.ts
  - tests/e2e-shell/theme-rotation.spec.ts
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-THEMEBRIDGE-TEST-TS-4fb837c0
  - TEST-OBS-SRC-LIB-COMPONENTS-CUSTOMTHEME-TEST-TS-0b73284d
  - TEST-OBS-SRC-LIB-MONACO-RIDGETHEME-TEST-TS-3b597c0b
  - TEST-OBS-SRC-LIB-STORES-THEMES-SLUG-TEST-TS-146943ed
  - TEST-OBS-SRC-LIB-STORES-THEMES-TEST-TS-8b9fb08d
  - TEST-OBS-SRC-REMOTE-LIB-THEME-TEST-TS-74f6feb0
  - TEST-OBS-TESTS-E2E-SHELL-THEME-INJECTION-SPEC-TS-ef7845cc
  - TEST-OBS-TESTS-E2E-SHELL-THEME-ROTATION-SPEC-TS-6e476756
---

# theme.rs

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
