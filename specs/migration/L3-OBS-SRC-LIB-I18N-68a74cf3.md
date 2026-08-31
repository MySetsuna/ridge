---
id: L3-OBS-SRC-LIB-I18N-68a74cf3
level: L3
parent: L2-OBS-SRC-25a66342
title: src/lib/i18n module
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/i18n/index.ts
  - src/lib/i18n/locale.ts
  - src/lib/i18n/messages.ts
public_interface:
  - "export function detectLocale(): Locale"
  - "export function setLocale(next: Locale): void"
  - "export function tr(key: string, vars?: TranslateVars): string"
  - "export function translate(loc: Locale, key: string, vars?: TranslateVars):
    string"
  - export type Locale
  - export type Region
  - export type TranslateVars
---

# src/lib/i18n module

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
