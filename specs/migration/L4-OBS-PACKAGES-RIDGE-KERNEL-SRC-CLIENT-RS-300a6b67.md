---
id: L4-OBS-PACKAGES-RIDGE-KERNEL-SRC-CLIENT-RS-300a6b67
level: L4
parent: L3-OBS-PACKAGES-RIDGE-KERNEL-SRC-fc45dbfb
title: client.rs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/ridge-kernel/src/client.rs
test_targets:
  - packages/ridge-kernel/src/client.rs
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-APICLIENT-REFRESH-TEST-TS-ae2beee9
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-APICLIENT-TEST-TS-54ddea70
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-RPCCLIENT-TEST-TS-605f379d
  - TEST-OBS-SRC-LIB-LSP-LSPCLIENT-TEST-TS-bab87361
---

# Kernel process exit evidence

The client preserves its authenticated, bounded Runtime requests and existing detached launch policy. Shutdown cleans the registry only after operating-system evidence proves process execution has ended. On Linux, a zombie or dead process state is terminated even while its PID awaits parent reaping; kill(pid,0) alone is insufficient. A stopped or permission-inaccessible process is not proven dead. Missing or malformed process-state evidence retains the existing conservative kill/EPERM result.

Liveness inspection must not reap a caller-owned Child. Detached children created by this launcher on Unix are reaped by their owning process after exit without coupling child lifetime to desktop exit. Windows process-tree flags and bounded termination behavior remain unchanged.

Acceptance: a real Linux child is deliberately left unreaped after exit, reports zombie state, and is classified dead while the caller can still wait successfully; a stopped live child stays alive. The existing real detached process-tree shutdown regression must pass on Linux and Windows. Run Kernel unit and Runtime integration gates, repository tests/lint/typecheck, and SpecTree verification. Release this correction as the next patch only after four-platform CI succeeds; retain the failed v0.1.91 tag as evidence.
