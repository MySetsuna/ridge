# Iteration 171 — Remote direct-link and input-first pane fluidity

## Scope

The approved requirement `REQ-REMOTE-LINK-FLUIDITY-01` targets the current
Remote failure mode: after switching Pane, the page can freeze at stale output
and reject input. Direct terminal links must open without first focusing or
opening the keyboard. The implementation keeps one authenticated transport and
adds bounded stage telemetry so a later public/mobile soak can distinguish
transport, host/relay, and device/render limits.

## Baseline and evidence

- Public Remote was checked at `https://s4host-s4test.9527127.xyz/`.
- The latest successful Remote/Cloud workflow was `30977176806`, commit
  `42680ca`, activating `0.1.60`; the deployed JavaScript still contains the
  old Ctrl-only link affordance and no current `openLinkAt`/first-paint path.
- The mobile terminal path injected a direct `TerminalManager` link handler
  only on the desktop path. Mobile therefore fell through to focus/selection
  behavior and could invoke keyboard/UI work before opening a link.
- Pane switching could synchronously drain a 4 MiB catch-up gap into the
  parser/renderer. A rapid switch made the main thread spend the entire turn
  replaying stale output, starving the active tail and input.
- Cloud active-pane QoS promotion was fire-and-forget for each subscription,
  allowing stale promotions to queue behind a newer Pane selection.

These facts establish a structural starvation defect. No runtime sample was
available to attribute remaining latency to public bandwidth, relay, host CPU,
or the phone GPU; the new trace is deliberately required before making that
claim.

## Implementation

- Bare primary clicks on validated URL/path hits open directly; non-link clicks
  retain TUI mouse/selection behavior. Hover keeps a thin continuous underline
  and shows `点击可跳转` without requiring Ctrl.
- Mobile mouse/touch hit testing calls the same `openLinkAt` route and avoids
  keyboard focus on successful link activation. Path opens cross the existing
  Remote file-viewer event bridge.
- Pane switch catch-up is bounded to 128 KiB per synchronous drain (2 MiB
  retained cap); overflow drops the oldest frames and requests one resync.
- Cloud active-pane promotion is serialized as latest-wins: one request in
  flight plus one pending target; stale fire-and-forget promotion storms are
  removed.
- `remotePerfTrace` records bounded, payload-free `input-rpc`, `resize-rpc`,
  `transport-send`, `raw-receive`, `raw-feed`, `pane-switch`, and
  `pane-first-paint` samples. It is off by default and can be enabled with
  `globalThis.__RIDGE_REMOTE_PERF_TRACE = true`, then inspected through
  `globalThis.__RIDGE_REMOTE_PERF.snapshot()`.
- Scheduler diagnostics now expose input/resize p50/p95 latency and input
  queue high-water bytes.

## Verification

- `pnpm check` — 0 errors, 0 warnings.
- Focused Remote/terminal suite — 8 files, 53 tests passed.
- `git diff --check` — passed (only expected LF/CRLF normalization notices).
- Full Vitest JSON reporter — 153 files, 1577 passed, 1 skipped, exit code 0.
- `pnpm build:remote:mobile` — production/PWA build passed; the desktop build
  also wrote `remote-dist/desktop` successfully (Vite emitted existing chunk
  split warnings through the PowerShell stderr wrapper).
- `pnpm check` — 0 errors, 0 warnings; iteration gate and requirements gate
  both report `ok: true`/`executable: true`.
- Public phone/PWA soak remains an external acceptance gate.

## Release status

Commit `e94d8c5` was pushed to `main`. Remote/Cloud workflow `30987238096`
completed successfully from that exact SHA, and the public entrypoint now
serves the new bundle (`openLinkAt` and `点击可跳转` are present; the old Ctrl-only
hint is absent). `/_app/version.json` reports `1785916897644`.

No Desktop version number was advanced: the formal Desktop release remains
`v0.1.60`. The requirement stays active until physical phone/PWA soak records
input latency, Pane-switch first paint, and the stage trace under the user's
real network.

## NLM note

The approved NotebookLM notebook was queried, but Google returned
`RESOURCE_EXHAUSTED` for this run. No new NLM note was created or marked as
complete; local CodeGraph/source/tests remain the authority for this code
slice.
