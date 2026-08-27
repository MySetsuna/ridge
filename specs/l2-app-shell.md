---
id: L2-APP-SHELL-001
level: L2
title: Web application shell
status: LOCKED
parent: L1-PROJECT-001
code_targets:
  - scripts/sync-generated-csp*
  - src-tauri/tauri.conf.json
  - src/hooks.server.ts
  - vite.config.js
test_targets:
  - scripts/sync-generated-csp.test.mjs
---

# Web application shell

Server hooks and Vite consume one generated CSP contract as a single owned
application boundary.
