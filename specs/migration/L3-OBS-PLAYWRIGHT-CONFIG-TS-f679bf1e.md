---
id: L3-OBS-PLAYWRIGHT-CONFIG-TS-f679bf1e
level: L3
parent: L2-OBS-PLAYWRIGHT-CONFIG-TS-f679bf1e
title: playwright.config.ts module
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - playwright.config.ts
---

Playwright browser E2E tests start the SvelteKit app in an isolated browser mode using Vite's e2e mode. The mode bypasses the native Runtime grant handshake so browser-only tests can exercise rendered UI without Tauri. Normal desktop and web-remote startup remain unchanged.
