# Iteration 85 Contract — Mobile Remote Worker Startup and Pane Lifecycle

## Scope

Close the approved mobile Remote stability slice without changing terminal
geometry or the main-thread rendering contract. The slice covers the render
worker cold-start race, pane init/bind ordering, worker replacement, pending
request cancellation, and deterministic mobile-console assertions.

Approved requirements carried forward:

- `REQ-MOBILE-REMOTE-KEYBOARD-QOS-02`
- `REQ-REMOTE-RUNTIME-PERF-MEMORY-02`
- `REQ-MOBILE-REMOTE-RUNTIME-LASTERROR-01`

## Root cause

The worker entry installed its `message` listener only after importing and
initialising the WASM adapter. A cold WebView2 start could therefore receive
the first pane `init` only after the host-side deadline, while a concurrent
fit sent `resize` before the worker had created the pane. A worker replacement
could also inherit stale pane IDs in the manager's attached set.

## Implemented

- Install the worker control-plane listener before WASM loading. `ping` is
  answered immediately; pane requests received while the adapter is loading
  receive one structured fallback error, allowing the host to restore the
  live main-thread canvas instead of waiting for a timeout.
- Pass the Vite-emitted WASM URL explicitly to the worker adapter. Keep a
  bounded 15-second `init` timeout separate from the 5-second hot request
  timeout, with pending-count and termination cleanup unchanged.
- Track `workerInitializing` separately from `workerAttached`. Suppress fit
  resize during init/bind, publish a pane only after the init acknowledgement,
  clear both sets on renderer replacement, and cancel in-flight handshakes on
  park/destroy. Late callbacks are accepted only from the current singleton.
- Extend the LAN mobile CDP probe to unregister stale ServiceWorkers and
  clear precache before the fresh navigation. The probe fails on worker init
  timeout, `resize before init`, hot worker timeout, or project
  `Unchecked runtime.lastError` output; it does not suppress other warnings.

## Verification

- `pnpm exec vitest run packages/remote/src/shared/terminal/workerHostedRenderer.test.ts packages/remote/src/shared/terminal/workerRendererBridge.test.ts`: 37 passed.
- Targeted Remote/Agent/keyboard/memory suite: 63 passed across 6 files.
- `pnpm check`: 0 errors, 0 warnings.
- Direct CDP worker probe: `ping` returned `{type:"pong",token:"probe"}`
  within the 3-second probe bound.
- `node scripts/cdp-remote-mobile-agents.mjs`: `GATE: PASS`; no forbidden
  lifecycle/runtime-messaging noise. Remaining logs are known dev WebSocket,
  Tauri callback, WebGPU fallback, and self-signed ServiceWorker environment
  warnings.

## Non-claims and external gates

This contract does not claim physical-phone clean-profile attribution,
third-party extension attribution, public Remote soak, WebView2 heap soak, or
dual-window/dual-host evidence. The source scan still finds no project Chrome
Extension Messaging (`chrome.runtime`/`chrome.tabs`/`sendResponse`) path; the
runtime.lastError requirement therefore remains an environment A/B gate, not
a Console-suppression task.

## Closure status

Deterministic code and local/CDP evidence are closed. Versioned release,
Remote artifact activation, ridge-cloud health, and the external gates remain
pending until their corresponding run IDs and assets are recorded.
