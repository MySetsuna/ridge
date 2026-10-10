---
id: L4-PERF-LATENCY-BENCH-ENTRY
level: L4
title: Latency benchmark command admission
status: LOCKED
lifecycle: ACTIVE
parent: L3-OBS-SCRIPTS-8c5967fd
code_targets:
  - scripts/bench-latency-4scenes.mjs
---

# Latency benchmark command admission

The existing four-scene diagnosis command owns its help entry. Passing --help prints supported invocation information and exits successfully before creating artifact directories, launching a host or browser, or changing certificate trust. This governance reconciliation only repairs help admission; real latency measurements remain subject to CHG-050/CHG-067 evidence requirements and are not established by the help check. Acceptance: node scripts/bench-latency-4scenes.mjs --help exits 0 without starting a measurement.
