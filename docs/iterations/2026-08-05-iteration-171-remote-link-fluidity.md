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

## Public path baseline (2026-08-05)

From this host, five HTTPS fetches of the public Remote root through the
configured HTTP proxy showed `1.58–34.63s` TTFB. Three direct (`--noproxy '*'`)
fetches showed `1.25–1.35s` TTFB. The deployed 531 KiB application chunk took
`2.05–2.65s` direct (`200–259 KB/s`) but `50.59–57.38s` through the proxy
(`9.3–10.5 KB/s`). This is strong evidence of a proxy/route bottleneck on this
machine, not proof of the Remote WebRTC media path: signaling may use the
proxy while the DataChannel may be direct, TURN-relayed, or host-limited. The
in-app `bufferedAmount` and stage trace remain authoritative for that split.

With the browser's `Accept-Encoding: gzip`, the same chunk compressed to
163,215 bytes: direct fetch `1.72s`; proxy fetch `16.60s` at `9.8 KB/s`.
Compression is working; the slow proxy route remains the dominant HTTP startup
cost.

## Network attribution from the cloud source

- `ridge-cloud/src/ws/handler.rs` routes signaling text (`offer`/`answer`/ICE)
  only; its own handler comment states that E2EE business payloads travel over
  WebRTC DataChannel and do not pass through the relay.
- `ridge-cloud/src/config.rs` enables TURN only when both `TURN_HOST` and
  `TURN_STATIC_AUTH_SECRET` exist; otherwise `/ice-servers` returns STUN only.
- The public asset response is `Server: nginx` with `Content-Encoding: gzip`
  and immutable cache headers. The measured 9.3–20.5 KB/s path is therefore
  not evidence of an application-side PTY bandwidth cap. The remaining
  authoritative split is the trace's WebRTC `candidateType`: `host`/`srflx`
  indicates a direct path, while `relay` makes TURN/server egress a possible
  bottleneck. Physical phone/PWA trace is still required before changing TURN
  capacity or buying device hardware.

## Remaining transport risk

The original `ordered` `ridge` DataChannel carried control/input JSON-RPC and
Pane output together. The local follow-up keeps `ridge` as the authenticated
control lane and adds an optional `ordered` `ridge-pane` bulk lane on the same
PeerConnection. Each lane has its own E2EE session, chunk-id space, and
backpressure watermark; old hosts/controllers fall back to `ridge`. This
removes transport head-of-line blocking without adding a second connection.

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
- Desktop foreign-pane bindings now mark the initial subscription active and
  promote the pane again on local focus, with duplicate promotions suppressed
  and reconnect state reset (`f5e9c2b0`). This closes the Host-topology path
  that could otherwise classify the focused pane as background and shed its
  live tail.
- Desktop foreign-pane bindings now retain an explicit attached-pane set and
  replay only that set after reconnect. A full WebRTC reconnect creates a new
  Host bridge, so replaying only the previous active promotion could leave
  live output unsubscribed; the focused pane is restored as `active:true`,
  while discovered-but-never-attached panes stay unsubscribed (`7bbcae00`).
- Mobile Cloud subscriptions now retry transient listener/registration
  failures with 100/200/400/800 ms backoff and a four-retry cap. Retry timers
  and intent state are cancelled on Pane destruction, disconnect, or reconnect;
  duplicate active requests still collapse into the existing promotion gate
  (`e8316548`).
- The transport slice is implemented locally (not yet published due today's
  release cap): Pane output is split into 32 KiB plaintext frames and routed
  to `ridge-pane` when available; control/input stays on `ridge`. The bounded
  priority queue, independent E2EE counters/chunk IDs, and 256 KiB/64 KiB
  high/low watermarks prevent a multi-megabyte PTY burst from holding a later
  keystroke behind ordered bulk traffic. Legacy peers transparently use the
  original lane.
- `remotePerfTrace` records bounded, payload-free `input-rpc`, `resize-rpc`,
  `transport-send`, `transport-stats`, `raw-receive`, `raw-feed`, `pane-switch`,
  and `pane-first-paint` samples. WebRTC stats include candidate type, RTT,
  available bitrate, bytes, and packet loss, with a one-second sample floor.
  It is off by default and can be enabled with
  `globalThis.__RIDGE_REMOTE_PERF_TRACE = true`, then inspected through
  `globalThis.__RIDGE_REMOTE_PERF.snapshot()`.
- Scheduler diagnostics now expose input/resize p50/p95 latency and input
  queue high-water bytes.

## Verification

- `pnpm check` — 0 errors, 0 warnings.
- Focused Cloud/transport suite — 6 files, 102 tests passed.
- `git diff --check` — passed (only expected LF/CRLF normalization notices).
- Full Vitest — 154 files, 1585 passed, 1 skipped, exit code 0 (including the
  priority transport and WebRTC stats guards).
- Reconnect subscription replay follow-up: full Vitest 154 files, 1586
  passed, 1 skipped; `pnpm check` 0 errors / 0 warnings.
- Rejected close rollback follow-up: full Vitest 154 files, 1587 passed, 1
  skipped; `pnpm check` 0 errors / 0 warnings. The subscription snapshot is
  restored when `closePane` or `closeWorkspace` fails.
- Mobile subscription retry follow-up: full Vitest 154 files, 1589 passed, 1
  skipped; `pnpm check` 0 errors / 0 warnings; `pnpm build:remote:mobile`
  passed with PWA precache 38 entries and `ridge-pane` probe/ready markers.
- `pnpm build:remote:mobile` — production/PWA build passed; the desktop build
  also wrote `remote-dist/desktop` successfully (Vite emitted existing chunk
  split warnings through the PowerShell stderr wrapper).
- `pnpm check` — 0 errors, 0 warnings; iteration gate and requirements gate
  both report `ok: true`/`executable: true`.
- `pnpm e2e:rdg-lan` (2026-08-05) — desktop and mobile browser clients both
  passed with `browserErrors=[]`; real WebSocket traffic included input and
  resize (`desktop write_to_pty=21`, `mobile write_to_pty=3`). Evidence:
  `.iteration/artifacts/rdg-remote-e2e/last-result.json`. This is controlled
LAN evidence, not a physical public WebRTC/PWA soak.
- Latest rerun at `2026-08-05T11:02:26Z` (evidence commit `4b8c76d2`) passed
  the same input/resize and browser-error gates. Raw detail records
  `desktop canvas=true tree=false ws=true` and `mobile canvas=true tree=true
  ws=true`; the acceptance gate does not require the Desktop tree flag, so this
  is recorded as-is rather than presented as a tree-pass.
- Public phone/PWA soak remains an external acceptance gate.

## Release status

Commit `e94d8c5` is the online artifact. Remote/Cloud workflow `30987238096`
completed successfully from that exact SHA, and the public entrypoint serves
the new bundle (`openLinkAt` and `点击可跳转` are present; the old Ctrl-only
hint is absent). `/_app/version.json` reports `1785916897644`. The local
priority transport follow-up is pushed as `67417a9` but is not online until
the next allowed artifact publish. The dual-lane implementation is pushed as
`7109c26` (superseding `67417a9`); controller outbound coverage and explicit
probe/ready legacy fallback are added in `321a5d3` and `150272a`; the focused
foreign-pane QoS promotion follows in `f5e9c2b0`.
Reconnect subscription replay follows in `7bbcae00`; it is also not online
until the next allowed artifact publish.
Rejected-close subscription rollback follows in `c9b8540d`; it is also not
online until the next allowed artifact publish.
Mobile subscription retry follows in `e8316548`; it is also not online until
the next allowed artifact publish.

No Desktop version number was advanced: the formal Desktop release remains
`v0.1.60`. The requirement stays active until physical phone/PWA soak records
input latency, Pane-switch first paint, and the stage trace under the user's
real network.

The priority transport change is deliberately not a Remote/Cloud artifact
today: the daily release cap is already exhausted. Online JavaScript therefore
remains `e94d8c5`; the next artifact must contain `e8316548` (including the
dual-lane `150272a` fix) and run the physical phone/PWA soak.

Live public fingerprint checked 2026-08-05: `/_app/version.json` still reports
`1785916897644`, and scanning the entrypoint's JavaScript assets finds no
`ridge-pane-probe`/`ridge-pane-ready` marker. The local `remote-dist/mobile`
build does contain those markers. Therefore the public Remote still uses the
legacy single ordered DataChannel, where a pane-output burst can hold later
input behind SCTP ordering; this is an artifact-activation gap, not evidence
against the dual-lane source implementation.

## NLM note

The approved NotebookLM notebook was queried, but Google returned
`RESOURCE_EXHAUSTED` for this run. No new NLM note was created or marked as
complete; local CodeGraph/source/tests remain the authority for this code
slice.
