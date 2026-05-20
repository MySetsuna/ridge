# tests/e2e-shell

True desktop e2e for Ridge (P3.14). Uses `tauri-driver` + WebdriverIO
to drive a real Tauri build, exercise real PTY + invoke surfaces, and
assert against the live wasm mirror via the `window.__windE2E` hook
that `manager.ts` installs at app startup.

## Setup (once per dev machine)

```powershell
# 1. Install WebdriverIO + Chai dev deps (committed package.json kept
#    lean — this tier is opt-in because the install is ~50 MB and
#    not needed for `pnpm e2e` / Playwright web-mode tests):
pnpm add -D @wdio/cli @wdio/local-runner @wdio/mocha-framework `
            @wdio/spec-reporter webdriverio @types/chai chai

# 2. Install tauri-driver from cargo:
cargo install tauri-driver

# 3. Build a release binary (the harness drives the .exe directly):
pnpm tauri:build
```

## Running

```powershell
pnpm e2e:shell
```

The harness spawns `tauri-driver --port 4444`, opens
`src-tauri/target/release/ridge.exe`, runs every `*.spec.ts` in this
directory, and tears the driver back down on completion.

## Specs

- `parserBackend.rust.spec.ts` — default 'rust' mode: feed PTY bytes
  via `window.__windE2E.feedPty`, assert mirror `visibleText` reflects
  the input.
- `parserBackend.wasm.spec.ts` — flip Settings.parserBackend to 'wasm'
  and repeat. Asserts both backends produce identical visible grids
  for the same byte stream.
- `parserBackend.switch.spec.ts` — toggle wasm ↔ rust five times with
  PTY traffic between each switch. Asserts the 200 ms fade mask
  appears + clears, and the mirror stays consistent across every
  switch (R4 verification).
- `resize.spec.ts` — programmatic window resize triggers `fitPane`;
  assert mirror grid dims match the PaneParser-driven Resize delta
  (R3 verification).

## Platform support

| Platform | Status |
|---|---|
| Windows | ✓ Primary target (WebView2 via msedgedriver) |
| Linux | ✓ webkit2gtk via WebKitWebDriver |
| macOS | ✗ Apple does not expose WKWebView WebDriver hooks |

If tauri-driver itself fails to connect on a fresh Tauri 2 build
(known caveat with older driver versions), fall back to
[WinAppDriver](https://github.com/microsoft/WinAppDriver) — same
WebDriver protocol, drop-in replacement on Windows.
