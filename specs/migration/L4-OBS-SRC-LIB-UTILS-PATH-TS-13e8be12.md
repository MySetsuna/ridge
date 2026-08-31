---
id: L4-OBS-SRC-LIB-UTILS-PATH-TS-13e8be12
level: L4
parent: L3-OBS-SRC-LIB-UTILS-2c402078
title: path.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/utils/path.ts
test_targets:
  - scripts/lib/toolPath.test.mjs
  - src/lib/utils/anchorRect.test.ts
  - src/lib/utils/ansi.test.ts
  - src/lib/utils/linkResolver.test.ts
  - src/lib/utils/linkTrust.test.ts
  - src/lib/utils/markdown.test.ts
  - src/lib/utils/path.test.ts
  - src/lib/utils/pathToken.test.ts
  - src/lib/utils/pLimit.test.ts
  - src/lib/utils/repeatedError.test.ts
  - src/lib/utils/resizeThrottle.test.ts
  - src/lib/utils/withTimeout.test.ts
public_interface:
  - "export function commonPathAncestor(paths: readonly string[]): string | null"
  - "export function isCurrentDirHref(href: string): boolean"
  - "export function isExternalUrl(href: string): boolean"
  - "export function isHomeRelative(href: string): boolean"
  - "export function isPosixAbsolute(href: string): boolean"
  - "export function isWindowsAbsolute(href: string): boolean"
  - "export function joinPath(base: string, rel: string): string"
  - "export function normalizePath(p: string): string"
  - "export function pathStartsWith(child: string, parent: string): boolean"
  - "export function stripQuery(pathPart: string): string"
  - "export function trimTrailingSeparators(value: string): string"
verified_by:
  - TEST-OBS-SCRIPTS-LIB-TOOLPATH-TEST-MJS-e11cd513
  - TEST-OBS-SRC-LIB-UTILS-ANCHORRECT-TEST-TS-105aff0a
  - TEST-OBS-SRC-LIB-UTILS-ANSI-TEST-TS-a521e4e0
  - TEST-OBS-SRC-LIB-UTILS-LINKRESOLVER-TEST-TS-a1aef792
  - TEST-OBS-SRC-LIB-UTILS-LINKTRUST-TEST-TS-a7833726
  - TEST-OBS-SRC-LIB-UTILS-MARKDOWN-TEST-TS-a9cb2814
  - TEST-OBS-SRC-LIB-UTILS-PATH-TEST-TS-82e03b6a
  - TEST-OBS-SRC-LIB-UTILS-PATHTOKEN-TEST-TS-b83f28c9
  - TEST-OBS-SRC-LIB-UTILS-PLIMIT-TEST-TS-47b9ca9a
  - TEST-OBS-SRC-LIB-UTILS-REPEATEDERROR-TEST-TS-16483412
  - TEST-OBS-SRC-LIB-UTILS-RESIZETHROTTLE-TEST-TS-34afb027
  - TEST-OBS-SRC-LIB-UTILS-WITHTIMEOUT-TEST-TS-02d14903
---

# path.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
