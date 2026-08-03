# Iteration 133 - OSC-8 link wrapping and LAN runtime evidence

Date: 2026-08-03

## Outcome

- OSC-8 links now expose a visual-row grid with explicit soft-wrap metadata.
  Ctrl/Cmd hover paints a 1px underline for every contiguous visual segment of
  the same logical target, while hard line breaks remain separate links. Copy
  and open resolution keep the logical target free of soft-wrap newlines.
- The controlled LAN desktop/mobile run passed the startup, terminal input,
  resize, workspace and Remote transport checks. The browser context disabled
  extensions and reported no page errors.
- Chromium mobile keyboard and touch-selection checks passed with bounded
  keyboard displacement, restored layout, and visible selection movement.
- Runtime `lastError` attribution remains deliberately conservative. The
  clean-profile probe is green but incomplete; project source has no
  `chrome.runtime.onMessage`/`sendMessage` implementation to patch or use as a
  Console suppression point.

## Evidence

- `pnpm exec vitest run packages/remote/src/shared/terminal/linkAffordance.test.ts packages/remote/src/shared/terminal/linkSpans.test.ts`: 2 files,
  17 passed.
- `pnpm check`: 0 errors / 0 warnings.
- `pnpm e2e:rdg-lan`: desktop and mobile `canvas/tree/ws=true`, input and
  resize requests observed, `browserErrors=[]`, `ALL PASS`.
- `pnpm e2e:rdg-mobile-keyboard`: `browserErrors=[]`; visual reduction,
  input-safe recovery, and touch-selection assertions all passed.
- `node scripts/remote-runtime-last-error-attribution.mjs`: clean-profile-only,
  `attributionComplete=false`, exit 0.
- Pushed commit: `e466812`.

## Boundaries and follow-up

This iteration does not claim a physical phone, public Remote path, WebView2
GPU-adapter selection, long-running WebView2 heap soak, or full Kernel/Tauri
authority migration. No version bump, release, Remote artifact, or Cloud
deployment was made because `v0.1.54` consumed today's publication allowance.
The next validation slice is physical/public runtime evidence, followed by the
remaining Kernel MCP/domain authority gaps without redirecting live PTY
commands prematurely.
