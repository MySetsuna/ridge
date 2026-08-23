---
id: L4-REMOTE-PERFORMANCE-GATE-001
level: L4
title: Verify Remote latency memory and cleanup
status: DRAFT
parent: L3-REMOTE-SMOOTHNESS-001
artifact: PSEUDOCODE
code_targets:
  - src/remote
  - packages/remote/src
test_targets:
  - tests/e2e
  - scripts/perf-bench.ps1
---

# Verify Remote latency memory and cleanup

```text
record client/host/relay latency, bytes, queue depth, CPU, and heap separately
soak reconnect, pane switch, history replay, background/resume, and IME input
assert queues and live timers/listeners/transports return to bounded baselines
compare public-network and physical-device results with the deterministic lab
leave the gate open when external evidence is absent
```
