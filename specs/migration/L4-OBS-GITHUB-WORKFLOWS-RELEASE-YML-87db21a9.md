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
  - scripts/build-ridge-kernel-sidecar.mjs
test_targets:
  - scripts/check-release-version.test.mjs
  - scripts/build-ridge-kernel-sidecar.test.mjs
verified_by:
  - TEST-OBS-SCRIPTS-CHECK-RELEASE-VERSION-TEST-MJS-ff79f177
---

# Release artifact integrity

Release runs build the exact tagged source after repository tests and version/tag consistency pass. Preserve the independently built ridge CLI in a dedicated staging path before building the same-named desktop binary. Upload only that staged CLI; verify its hash has not changed and verify native CLI command behavior (cross builds verify architecture). Never fall back to the desktop output as a CLI asset.

Windows, Linux, macOS ARM64 and macOS x64 assets must finish successfully before publication. Keep generated releases as drafts while build/asset checks run. Publish the verified draft only under the user's release authorization, with notes describing the actual delivered Runtime fixes. A failed platform or mismatched artifact blocks publication.

Release tests must run Kernel unit and real runtime_workspace_continuity, cwd_runtime, and workspace_file_owner integration tests on Linux and Windows. Windows must exercise the PowerShell cwd/split case; ignored subprocess fixtures stay excluded from direct execution. Both jobs must check out the requested release tag, including workflow_dispatch.

Native compile checks must prepare every declared Tauri input before cargo check: build and verify actual ridge-mcp and ridge-kernel sidecars, and build the actual remote UI resource bundle. Reuse existing builders, with debug Kernel profile for compile checks; never bypass resource checks or create placeholders. Kernel packaging must honor the target triple supplied by Tauri hooks, while preserving explicit --target and CARGO_BUILD_TARGET overrides and ordinary host fallback. Cross-compilation must stage the target Kernel alongside any host helper, without globally forcing Cargo's target on Wasm builds. Acceptance covers target precedence, real Linux native compile, and all four release builds/installer checks. Earlier failed tags stay unchanged.

Release shell logging must remain portable to macOS Bash, including nounset mode: use a fixed printf format with separately quoted arguments so adjacent punctuation cannot alter variable names. Logging success must not turn a completed upload into a failed job. Validate shell syntax and actual release completion; advance the product to 0.1.96 after the blocked 0.1.94/0.1.95 attempts.
