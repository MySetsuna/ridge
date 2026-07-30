# Iteration 74 Contract — CDP development acceptance matrix

- Date: 2026-07-29
- Status: accepted
- Scope: approved user-track requirements through iteration 73

## Runtime boundary

1. Preserve the installed Ridge host. An isolated Dev Ridge may be started,
   restarted, and stopped through the existing CDP scripts.
2. Exercise desktop WebView, LAN Remote, public Remote, and mobile emulation
   where targets and authentication already exist.
3. Record DOM geometry, renderer/kernel dimensions, request/console failures,
   interaction state, and screenshots only when they prove a requirement.
4. Per user approval, the established Dev production-simulation scripts count
   as acceptance evidence for browser, mobile, PowerShell, and account paths.
5. Any runtime defect becomes a focused follow-up iteration before more broad
   validation.

## Acceptance groups

- Remote geometry, pointer mapping, resize, reconnect, background continuity.
- Mobile viewport, keyboard anchoring, overlays, icon-only controls.
- Explorer resize, context actions, cut/copy failure reporting.
- Agent Commune state, history, MCP submit, and Ridge-owned native sessions.
- Terminal history overlay, focus/cursor stability, and render diagnostics.

## Gates

- No uncaught console errors attributable to the tested path.
- Geometry assertions use DOM rect plus renderer/kernel rows and columns.
- Cross-workspace actions retain explicit workspace/pane identity.
- NotebookLM remains exactly two sources.
- Versioned release is complete only after the tag-matching Windows, Linux,
  and macOS asset classes exist on GitHub.

## Accepted evidence

- Isolated WebView2 CDP selected a dynamic debug port and a reserved Ridge
  Vite port without touching the installed Ridge host or the unrelated service
  already using port 5173.
- PTY parsers, LAN workspace routing, pane graph broadcasts, directory
  pagination, Commune MCP submit receipts, agent discovery/panel controls,
  mobile Remote, and desktop Remote continuity passed in the Dev host.
- Explorer accepted arbitrary resize deltas (`288 → 361 → 320 px`).
- History Overlay changed only the active pane canvas, retained pane geometry
  and focus, and passed all three renderer containment/DPR tests.
- Deterministic gates passed: `svelte-check` (0 diagnostics), Vitest
  (110 files; 1275 passed, 1 skipped), ridge-core git/process guard
  (34 passed), and unified Remote desktop/mobile production build.
