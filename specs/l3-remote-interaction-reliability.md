---
id: L3-REMOTE-INTERACTION-RELIABILITY
level: L3
title: Remote desktop and mobile interaction and runtime reliability
status: LOCKED
lifecycle: ACTIVE
parent: L2-REMOTE-001
code_targets:
  - src/remote/lib/TerminalCanvas.svelte
  - src/remote/MainApp.svelte
  - src/remote/lib/paneFeedScheduler.ts
  - src/remote/lib/remoteInput.ts
  - src/remote/lib/retryableLazyImport.ts
  - packages/remote/src/shared/terminal/manager.ts
  - packages/remote/src/shared/terminal/mouseForwardPolicy.ts
  - packages/remote/src/shared/transport/paneRpcScheduler.ts
  - packages/remote/src/shared/transport/wsRemote.ts
  - packages/remote/src/shared/cloud/priorityFrameQueue.ts
  - packages/remote/src/shared/cloud/controllerCloudProvider.ts
  - scripts/tauri-dev-cdp.mjs
  - src/remote/App.svelte
  - vite.remote.config.js
  - packages/ridge-term/src/lib.rs
  - packages/ridge-term/src/term/terminal.rs
  - packages/ridge-term/src/term/grid.rs
  - packages/ridge-term/Cargo.toml
  - packages/ridge-term/build.mjs
  - packages/ridge-term/src/render/glyph_rasterizer.rs
  - packages/ridge-term/src/render/gpu_context.rs
  - packages/ridge-term/src/render/mod.rs
  - packages/ridge-term/src/render/renderer.rs
  - packages/ridge-term/src/render/webgpu.rs
  - packages/ridge-term-render/Cargo.toml
  - packages/ridge-term-render/src/lib.rs
  - packages/ridge-term-font/Cargo.toml
  - packages/ridge-term-font/src/lib.rs
  - packages/remote/src/shared/terminal/wasmRuntime.ts
  - src-tauri/src/state.rs
  - Cargo.toml
  - Cargo.lock
  - package.json
  - pnpm-lock.yaml
  - vite.config.js
  - scripts/build-remote-desktop.mjs
  - packages/ridge-remote/src/ua.rs
  - packages/ridge-remote/src/serve.rs
  - packages/ridge-remote/src/embed_ui.rs
  - packages/ridge-remote/build.rs
  - packages/remote/src/shared/cloud/cloudHostBridge.ts
  - packages/remote/src/shared/cloud/cloudHostPaneSource.ts
  - scripts/sync-signaling.mjs
  - src/remote/lib/cloudRemote.ts
  - src/remote/AuthScreen.svelte
  - src/remote/lib/remoteSession.ts
  - src/remote/CloudAuthScreen.svelte
  - packages/ridge-term/src/term/scrollback.rs
  - packages/ridge-term/src/selection.rs
  - packages/ridge-term/src/search.rs
  - packages/ridge-term/src/semantic.rs
  - packages/ridge-term/src/terminal_protocol_generated.rs
  - packages/ridge-cli/src/kernel_host_impl.rs
  - packages/ridge-cli/src/rpc.rs
  - src-tauri/src/lib.rs
  - packages/remote/src/shared/cloud/signaling/generated/**
  - packages/remote/src/shared/cloud/signaling/fixtures/**
  - packages/remote/src/shared/cloud/signaling/SOURCE_REV
  - packages/remote/src/shared/transport/rpcClient.ts
  - docs/archive/CHG-069/**
  - packages/remote/src/shared/cloud/signaling/index.ts
  - packages/ridge-term/src/producer.rs
  - packages/ridge-term/src/term/delta.rs
  - packages/remote/src/shared/terminal/ptyBridge.ts
  - packages/ridge-term/src/*.rs
  - src/lib/hosts/hostForest.ts
  - packages/ridge-term-font/src/*.rs
  - packages/remote/src/shared/terminal/retryableLazyImport.ts
  - packages/ridge-term-font/pkg/**
  - packages/ridge-term/src/term/attr_table.rs
  - packages/ridge-term/src/term/attrs.rs
  - packages/ridge-term-render/src/*.rs
  - packages/ridge-term-render/pkg/**
  - src/lib/transport/tauriShim/bridge.ts
  - src/lib/transport/tauriShim/core.ts
  - packages/remote/src/shared/transport/lanWsAdapter.ts
  - packages/remote/src/shared/transport/types.ts
  - packages/remote/src/shared/transport/cloudWebrtcAdapter.ts
  - src/routes/+layout.svelte
  - src/lib/hosts/remotePaneBindings.ts
  - packages/remote/src/shared/transport/cloudMux.ts
  - src/lib/remote/cloud/cloudHostStore.ts
  - packages/ridge-kernel/src/domain.rs
  - packages/ridge-kernel/src/server.rs
  - packages/ridge-core/src/capability.rs
  - packages/remote/src/shared/cloud/remoteAllowlist.ts
  - .github/workflows/release.yml
  - .github/workflows/rtp1-kernel-e2e.yml
test_targets:
  - src/remote/lib/TerminalCanvas.test.ts
  - src/remote/lib/remoteInput.test.ts
  - src/remote/lib/retryableLazyImport.test.ts
  - src/remote/lib/paneFeedScheduler.test.ts
  - packages/remote/src/shared/terminal/manager.attach.test.ts
  - packages/remote/src/shared/transport/paneRpcScheduler.test.ts
  - packages/remote/src/shared/transport/wsRemote.behavior.test.ts
  - scripts/remote-interaction-acceptance.mjs
  - scripts/tauri-dev-cdp.test.ts
  - packages/remote/src/shared/terminal/manager.test.ts
  - packages/remote/src/shared/terminal/wasmRuntime.test.ts
  - scripts/terminal-five-fixes-e2e.mjs
  - scripts/terminal-system-acceptance.mjs
  - packages/ridge-remote/tests/ua_fork_serve.rs
  - src/remote/lib/cloudRemote.test.ts
  - src/remote/MainApp.test.ts
  - packages/remote/src/shared/terminal/managerPerfBaseline.test.ts
  - src/remote/lib/remoteSession.test.ts
  - packages/remote/src/shared/cloud/signaling/drift.test.ts
  - packages/remote/src/shared/cloud/cloudHostBridge.test.ts
  - packages/remote/src/shared/cloud/cloudHostPaneSource.test.ts
  - packages/remote/src/shared/transport/conformance.test.ts
  - packages/remote/src/shared/transport/cloudWebrtcAdapter.test.ts
  - packages/remote/src/shared/transport/wsRemoteRpcScheduler.test.ts
  - packages/remote/src/shared/terminal/ptyBridge.test.ts
  - src/lib/transport/tauriShim/bridge.test.ts
  - src/lib/transport/tauriShim/compatibility.test.ts
  - packages/remote/src/shared/transport/lanWsAdapter.test.ts
  - src/lib/hosts/remotePaneBindings.test.ts
  - scripts/terminal-history-session-acceptance.mjs
  - packages/remote/src/shared/cloud/ridgeCloudProvider.test.ts
  - packages/remote/src/shared/cloud/controllerCloudProvider.test.ts
  - packages/remote/src/shared/transport/cloudMux.test.ts
  - src/lib/remote/cloud/cloudHostStore.test.ts
  - packages/remote/src/shared/cloud/weakNetLab.test.ts
  - packages/remote/src/shared/cloud/faultInjection.test.ts
  - scripts/cloud-terminal-system-acceptance.mjs
  - packages/remote/src/shared/cloud/remoteAllowlist.test.ts
  - packages/ridge-kernel/tests/remote_invoke_runtime.rs
---

# Remote interaction and runtime reliability

## Scope

Rebuild terminal presentation and semantic delivery as one desktop/Remote contract, authorized by the user on 2026-10-11. Preserve terminal behavior, session ownership, auth, encryption and persistence boundaries. Archive superseded terminal-v2; do not ship parallel terminal protocols or a raw-history replay fallback.

## CHG-069 / CHG-070 responsibility boundary

CHG-069 owns portable terminal behavior and its real desktop/Remote integration: cursor presentation, link detection/hit testing/GPU affordances, negotiated keyboard/pointer input, ordered bounded client delivery and recovery, semantic-row history loading, functional WASM demand loading, and default simple Remote UI with explicit ui=desktop opt-in. These capabilities remain implemented and verified within CHG-069; none is deferred merely because CHG-070 will change host wiring.

CHG-070 owns desktop presentation-only conversion: extracting domain dispatch, business state, persistence and background services from Tauri; making desktop, CLI and Remote use one authorized Host request/event path; independent runtime discovery, packaging and lifetime; and archiving the old DesktopHost/Tauri business chain. CHG-069 does not perform this migration, create a second domain service, change process ownership, or promise service survival after desktop exit.

Host/parser/transport paths can be touched sequentially by both changes without owning the same requirement. In CHG-069, host changes are the minimum adapters needed to deliver authoritative semantic rows and repair ordered input/output; reusable terminal logic belongs to ridge-term or existing independent Kernel/core services. Do not add new desktop-owned history, parser, dispatcher or persistence authorities, and do not duplicate an implementation in LAN, Cloud and Tauri handlers. Existing adapters call the shared implementation. In CHG-070, migrate or replace those adapters while retaining the same terminal contract and regression suite; do not independently redefine link matching, cursor timing, semantic rows or WASM loading.

No current file path freezes the final architecture. A 069 temporary adapter is acceptable only when it wraps the shared implementation and its later replacement changes connection/lifecycle rather than terminal semantics. Kernel/private credentials, auth, workspace/pane/epoch/activation fences and the protocol's authoritative schema remain binding in both changes. Any required new wire semantics must go through the existing protocol source and negotiation; neither change owns a parallel wire schema.

069 acceptance proves terminal behavior and loading/input/rendering metrics on the current supported real entry paths. 070 acceptance proves unified business ownership, capability parity, runtime survival and migration correctness, then reruns the 069 contract tests against its new connections. Complete 069 before authorizing 070. Path overlap is a sequential handoff, never concurrent implementation authorization or permission to reset 069's evidence. Completing either requires its actual requirements, not merely a passing generic build gate.

## Input fidelity

Use the terminal kernel's negotiated encoder for keyboard chords, special keys, paste and mouse reports. A physical action reaches the host once, with modifiers, row/column, pressed button and release intact. Support physical and virtual keyboards, IME ordering, mobile beforeinput deletion/newline, composition commits, drag/release outside the surface, pointer cancellation, mouse modes 1000/1002/1003, touch scrolling and explicit local selection. Local selection, links, browser clipboard and program mouse ownership must not double forward. Never replay uncertain input across panes or host epochs. Binding transitions and error states remain visible.

## Bounded delivery and recovery

Keep input ordered and admitted queues bounded. Output processing yields within byte/time budgets, active-pane work cannot starve other panes, and background/hidden-page scheduling must recover. Preserve stream and semantic-delta order; truncation must cause authoritative resync rather than silently corrupt parser state. Disconnect, slow ACK, floods, resize storms and rapid switching cannot produce unbounded pending work or permanent busy states. Timeouts and failures are observable and recoverable. Measure before/after for performance changes with fixed scenarios; do not infer responsiveness from queue limits alone.

## Demand loading

Terminal runtime loads only when needed; optional sidebar, roster, file viewer and keyboard chunks load on first use. Failed lazy requests present retryable errors and do not permanently cache a rejected promise. Mount/unmount, reconnect and switching must release event listeners, timers, pending tasks and obsolete callbacks while retaining legitimate pane state.

## Acceptance

Reproduce each fix against real component behavior and terminal encoders, then regress input bytes and visible restoration. Use tauri:dev:cdp for the current source host and CDP desktop/mobile browser contexts against its authenticated Remote route. Record loaded frontend/host build and transport, exercise modifiers, IME, paste, mouse motion/drag/release/cancel, touch scroll, switching, disconnect/reconnect, sustained output plus typing and failed lazy loads. Browser mobile emulation is distinct from physical-device evidence. Capture metrics and screenshots without credentials or private terminal payloads. Mark inaccessible real WAN or physical-device scenarios explicitly, never claim them covered by localhost mocks. Run npm test, lint, typecheck, consumed frontend/WASM builds, relevant native checks, then stc verify and complete. Broad usability remains incomplete if input or recovery scenarios are unverified.

Development acceptance must launch the current embedded Kernel without requiring a removed legacy CLI binary, keep generated state under existing local runtime directories, and proxy LAN development requests using the actual HTTPS/WSS host transport without weakening production certificate or authentication policy.

## Shared terminal ownership and row demand loading

CHG-069 includes the user's cursor, link, Remote transport/rendering and WASM split requests (2026-10-10). Desktop and Remote use ridge-term as the authoritative terminal kernel for cell coordinates, negotiated input, logical link spans and row presentation. The cursor/link behavior is defined by L3-TERMINAL-VISUAL-INPUT-REPAIR; adapters supply host opening and surface lifecycle rather than defining independent matching or rendering rules.

Remote terminal loading is by semantic rows, not raw historical byte replay or frontend chunk loading alone. Seed only the required visible rows and bounded recent context; fetch older semantic row pages when the viewport approaches unloaded history. Preserve cells, graphemes, attributes, soft wrapping and OSC8 metadata exactly. Keep active-pane live deltas ordered; inactive panes do not receive background streams. History loading neither reparses old escape sequences into the live terminal nor changes live cursor, modes or ownership. Pages are bounded, deduplicated and fenced by pane, activation and host epoch; late or duplicate pages cannot corrupt another pane, and failure retains existing rows with observable retry. Preserve the user's viewport anchor when inserting history. Handle unavailable/evicted history and revision gaps explicitly through authoritative recovery. Use the single rebuilt terminal contract and generated types; any needed schema delta must originate from its single authoritative source and be negotiated consistently by all transports.

## WASM package split

The terminal WASM must be delivered as independently loadable functional modules; merely renaming the existing monolith or adding a JavaScript import wrapper does not satisfy this request. Keep the lean terminal kernel usable independently of GPU/font rendering dependencies; load the required rendering/font/backend modules on demand. Maintain one logical terminal state and the existing shared GPU surface model, with no full-grid serialization or duplicate parsing on each frame. Respect module-instance memory/object ownership, version compatibility, retryable initialization and teardown across desktop and Remote. Do not eagerly fetch unused backend or optional modules. Keep current Unicode, font quality, WebGPU/WebGL2 availability, input and terminal semantics. Inspect release artifacts as well as development artifacts before attributing size or latency.

## Extended acceptance

Use a common desktop/Remote fixture suite covering held navigation keys with continuous output, cursor-only moves, explicit synchronized-output transactions, wrapped URLs and paths, OSC8, wide characters, viewport/resize/switch changes and plain prose or ratios. Prove every browser presentation frame with an authoritative cursor move repaints at its current cell; a quiet timer cannot hide a held-key sequence. Prove link underlines are limited to the validated hovered target with the existing modifier gesture, while SGR text underline retains its normal semantics. Validate both GPU backends and trusted origin opening. Regress incorrect candidates such as ratios, MIME types, prose containing slash and balanced URL punctuation.

Exercise semantic row loading with long history, scroll/output races, fast pane switching, reconnect/epoch changes, expired history, repeated pages and failing requests, including real LAN and Cloud paths. Measure transferred bytes, loaded rows, queue occupancy, frame responsiveness, input-to-paint latency and bounded memory under fixed comparable workloads. For WASM splitting, record each release module's raw/compressed bytes, actual network requests, compile/init time and first usable terminal paint under cold/warm cache and constrained-network browser runs. Demonstrate a reduction in the required initial payload and loading time against the existing release monolith, without moving cost into an unmeasured mandatory second load. Rebuild consumed WASM/frontend assets, run repository and native checks and deterministic stc verify/complete. Unit or local-only evidence must not be reported as full device/WAN acceptance.

## Remote UI entry

All Remote browser entries, including LAN and Cloud on desktop and mobile devices, default to the current simple mobile-style interface. Only an explicit URL query parameter ui=desktop selects the desktop interface. User agent, viewport width, cached preference and previous visits do not select desktop implicitly. Missing, invalid or ui=mobile values retain the simple interface. Direct navigation, deep links, index.html, reload and service-worker handling must preserve this selection, while each shell can load its own assets without requiring the ui query on every asset request. Native desktop startup retains its existing UI. Exercise desktop and mobile browser contexts for default, explicit desktop and invalid queries on real current-build LAN and Cloud serving paths; existing implementations need verification rather than duplication.


## Foreground reconnect and retained sessions

CHG-069 owns mobile background/foreground reconnect and reload without verification when the existing Remote session remains valid. CHG-070 migrates service ownership and reruns this contract; it does not postpone or independently redesign these repairs. Foreground resume and online events must recover closed, half-open and stuck CONNECTING sockets within bounded attempts, fence obsolete callbacks, restore authoritative active-pane state, and never replay uncertain input. Transport owns socket retries; authentication UI must not run a competing retry loop.

Persist the existing issued session credential using browser storage with its existing host/device scope. Reload reuses it before offering verification. Transient network failure, timeout, suspension or unavailable session checks retain the credential and show a retryable connection failure; only authoritative expiry/revocation/auth rejection permits invalidating it and requesting verification. Session validity and lifetime remain server-owned; do not invent client lifetimes, extend expiry, remove device/IP binding or persist host session registries as part of this repair. Cloud uses its existing authenticated trust/session restoration mechanism; never persist one-time codes as session credentials.

Acceptance covers background/foreground with a silently dead OPEN socket, stuck handshake, offline/online, same-origin reload, transient failed validity checks and authoritative invalid sessions. Valid-session reconnect/reload must issue no new verification request, reach a usable active pane and preserve identity; invalid-session recovery must remain bounded and visibly request verification. Record browser-emulated suspension separately from physical mobile evidence.


## Single terminal contract rebuild (2026-10-11)

The user explicitly rejected restoring a v2 layer and authorized rebuilding the terminal system. The canonical portable wire schema and request types live only in ridge-signaling/src/terminal.rs; wind consumes byte-identical generated Rust/TypeScript and golden fixtures. Archive the superseded codec/adapters, remove their active imports/method names, and use one terminal contract in desktop, LAN and Cloud. Wire versions identify incompatible byte shapes and are negotiated deterministically; they never select a legacy behavior path. Incompatible peers show an explicit protocol error instead of replaying VT history. CHG-070 inherits the contract and only migrates service ownership.

Frames carry pane, terminal lifetime and controller activation. Live revisions are contiguous; duplicate/stale frames are harmless, gaps require an authoritative snapshot. History pages use absolute row ranges and a history generation changed on clear/reflow; ordinary live output cannot invalidate a page solely by advancing its live revision. Snapshot, delta and history packets each carry their own bounded OSC8 dictionary; cells do not depend on an unbounded persistent wire-ID registry. Late pages, overlapping ranges and missing definitions never partially mutate the live terminal. Preserve viewport/selection anchors and explicitly handle expired/evicted ranges.

Initial delivery contains visible screens and a bounded recent row tail. Fetch older rows only near the unloaded boundary. No full-history snapshot on ordinary output, sandbox replay of history, or silently truncated stream. Encoded input crosses transports as bytes with source sequence and terminal-lifetime fencing, preserving non-UTF8 mouse reports. Capture, encoding, admission and host acknowledgement use the same desktop/Remote path. Record real integration and row/payload/paint metrics before completion.

## Repeated recovery and Host negotiation

A second connected edge during in-flight reauthorization retains the latest recovery intent. Authorization results from an obsolete connection generation cannot change the new link state, tear down its subscriptions or notify a successful recovery. Once the obsolete work settles, process the latest still-connected generation exactly once, retaining trust-grant/TOTP policy and never replaying uncertain terminal input. Final disconnect disposes all pending recovery work.

Kernel answers the existing JSON-RPC notification hello with the canonical negotiated hello or rejection notification, as required by the current protocol implementation. Supported client notification shapes, including unsubscribe after native JSON-RPC promotion, reach the same subscription lifecycle and authorization checks as their existing forms. No new protocol schema or privilege is introduced. Canvas context-loss/restoration is observed at the actual canvas propagation boundary so WebGL fallback can recover.

Acceptance includes deferred authorization while the connection repeatedly drops and recovers, obsolete successes/failures and final disconnect; real Kernel WebSocket hello acceptance/rejection and unsubscribe stopping live delivery; desktop/mobile browser restoration and bounded local latency measurements with exact build/route and explicit real-device/WAN gaps. Unit-only transport tests do not establish overall network or mobile performance.

Continuous validation runs current independent Kernel, real Commune registration/group-task and Remote invoke/notification integration tests on ordinary changes as well as releases. Legacy desktop/CLI launch harnesses are archived when replaced; CI artifacts must not upload endpoint files or tokens. Regression fixtures use owned settings/cwd/auth roots and assert metadata-plus-snapshot ordering from the current stream contract.

When enabling LAN Remote without an explicit port, retain the existing default-port probing and fall back to an OS-assigned available port if that range cannot bind. Report and advertise the actual port through the existing info/QR/mDNS path. An explicitly requested port never silently changes outside its existing probe range. Keep native-only controls, TLS, authentication and listener cleanup unchanged. Verify default failure/recovery and explicit-port failure policy, and exercise the owned current-source host on Windows where the default range is reserved.
