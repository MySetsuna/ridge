---
id: L4-OBS-SRC-LIB-I18N-LOCALE-TS-bc1e8777
level: L4
parent: L3-OBS-SRC-LIB-I18N-68a74cf3
title: locale.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/i18n/locale.ts
public_interface:
  - "export function detectLocale(): Locale"
  - "export function setLocale(next: Locale): void"
  - export type Locale
  - export type Region
---

# locale.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
