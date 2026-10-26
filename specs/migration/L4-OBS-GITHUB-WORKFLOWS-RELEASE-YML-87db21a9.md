---
id: L4-OBS-GITHUB-WORKFLOWS-RELEASE-YML-87db21a9
level: L4
parent: L3-OBS-GITHUB-WORKFLOWS-87ee5504
title: release.yml
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - .github/workflows/release.yml
test_targets:
  - scripts/check-release-version.test.mjs
verified_by:
  - TEST-OBS-SCRIPTS-CHECK-RELEASE-VERSION-TEST-MJS-ff79f177
---

# Release artifact integrity

Release runs build the exact tagged source after repository tests and version/tag consistency pass. Preserve the independently built ridge CLI in a dedicated staging path before building the same-named desktop binary. Upload only that staged CLI; verify its hash has not changed and verify native CLI command behavior (cross builds verify architecture). Never fall back to the desktop output as a CLI asset.

Windows, Linux, macOS ARM64 and macOS x64 assets must finish successfully before publication. Keep generated releases as drafts while build/asset checks run. Publish the verified draft only under the user's release authorization, with notes describing the actual delivered Runtime fixes. A failed platform or mismatched artifact blocks publication.

Release tests must run Kernel unit and real runtime_workspace_continuity, cwd_runtime, and workspace_file_owner integration tests on Linux and Windows. Windows must exercise the PowerShell cwd/split case; ignored subprocess fixtures stay excluded from direct execution. Both jobs must check out the requested release tag, including workflow_dispatch.
