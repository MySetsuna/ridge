---
id: L4-OBS-PACKAGES-RIDGE-CORE-SRC-SANDBOX-RS-56a1db98
level: L4
parent: L3-OBS-PACKAGES-RIDGE-CORE-SRC-036e205f
title: Filesystem root containment and Windows path identity
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/ridge-core/src/sandbox.rs
test_targets:
  - packages/ridge-core/src/sandbox.rs
  - packages/ridge-kernel/tests/runtime_workspace_continuity.rs
---

# Filesystem root containment

RootScope governs operator and client filesystem root boundaries shared by Core command dispatch and Kernel observations. Empty roots keep their existing unrestricted meaning; configured roots enforce containment, component boundaries, traversal handling and the existing canonical check against symlink escape.

On Windows, an absolute candidate whose lexical spelling differs from absolute configured roots may be admitted only when filesystem resolution of both candidate (or nearest existing ancestor plus missing tail) and an allowed root proves physical containment. This includes 8.3 aliases and canonical Windows prefixes. Failed resolution cannot authorize a lexical mismatch; physical targets outside every root remain rejected. Preserve the existing lexical fallback for unresolved paths already lexically inside a root. Other platforms keep existing admission rules.

Acceptance covers both directions of short/long root identity, existing files and not-yet-created descendants, rejection of outside siblings and link/junction escapes including missing leaves, plus the real short-TEMP Kernel PowerShell cwd/split/save integration. Keep both operator and caller-root checks; do not change permission scope to unrestricted.
