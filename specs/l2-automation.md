---
id: L2-AUTOMATION-001
level: L2
title: Project automation and verification
status: LOCKED
parent: L1-PROJECT-001
depends_on:
  - L2-APP-SHELL-001
  - L4-REMOTE-GPU-COMPAT-001
code_targets:
  - scripts/**
test_targets:
  - scripts/**/*.test.mjs
  - scripts/**/*.test.ts
---

# Project automation and verification

Build, CDP, performance, and release scripts consume product contracts without
making the root project node a synthetic runtime dependency. Evidence scripts
validate stable machine-readable scope fields rather than localized prose and
spawn argument-bearing tools without a command shell.
