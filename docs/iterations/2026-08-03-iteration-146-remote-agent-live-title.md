# Iteration 146 — Remote Agent live Pane title

Date: 2026-08-03

## Scope

Keep Remote Agent cards aligned with the desktop PaneHeader title. Stable Agent
identity remains the action/history key; the live title is presentation data
and may change as the PTY emits OSC title updates.

## Implementation

- `TeammateRosterMember` now carries optional `title` from the host topology
  projection.
- `SidebarTeamRoster` renders `title.trim()` when present and falls back to
  the stable Agent name. CWD and pane identity remain separate fields.
- The existing backend `inject_roster_titles` path is reused; no second title
  polling request or identity matching heuristic was added.

## Evidence

- `pnpm exec vitest run src/remote/lib/SidebarTeamRoster.test.ts src/remote/lib/sidebarProvider.test.ts --reporter=dot`
  — 24 passed.
- `pnpm check` — 0 errors, 0 warnings.

## Boundaries

This closes the Remote title projection gap only. Physical mobile/public
evidence, WebView2 heap soak, and full Kernel authority remain open; no release
or version bump is made.
