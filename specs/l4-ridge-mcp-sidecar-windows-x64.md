---
id: L4-RIDGE-MCP-SIDECAR-WINDOWS-X64
level: L4
title: ridge-mcp Windows x64 sidecar artifact
status: LOCKED
lifecycle: ACTIVE
parent: L3-OBS-PACKAGES-RIDGE-MCP-BRIDGE-ec7d0640
code_targets:
  - src-tauri/binaries/ridge-mcp-x86_64-pc-windows-msvc.exe
test_targets:
  - packages/ridge-mcp-bridge/src/main.rs
---

# ridge-mcp Windows x64 sidecar artifact

The Windows x64 desktop release bundles the ridge-mcp executable built from packages/ridge-mcp-bridge as a Tauri sidecar. The generated artifact is tracked as an exact release input so SpecTree detects and authorizes sidecar refreshes instead of treating them as unexplained workspace drift.
