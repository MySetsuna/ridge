# Iteration 166 — Remote/WebRTC process boundary and hard-kill continuity

## Scope

Close the remaining gap from iteration 165: hiding the Tauri window was not
enough. A force-killed desktop process must leave the kernel PTYs and Remote
transport usable, and a later desktop start must reattach instead of creating a
second local transport or PTY owner.

## Implementation

- Added the Tauri-free `rdg host` process. It serves LAN Remote/WebRTC-facing
  protocol through `KernelHost`, while workspace identity, PTY state and output
  leases stay in the detached `ridge-kernel`.
- Added Tauri `remote_host_supervisor`: starts/reuses detached LAN and cloud
  sidecars, persists PID/port/enable registries, synchronizes cloud credentials
  through an app-data file, and reattaches only previously enabled cloud hosts.
- Native `cloudHostStore` no longer creates `RidgeCloudHost` in the WebView;
  browser/PWA retains its fallback implementation. Desktop online/offline now
  invokes the sidecar lifecycle commands.
- `PtyBridge` prefers the kernel and attaches an output lease without destroying
  the kernel PTY on WebRTC disconnect. Existing PTYs are selected by CWD when
  available; reconnect leases start at the current output head to avoid replay
  storms.
- Windows detached spawn requires `CREATE_BREAKAWAY_FROM_JOB` in production;
  only constrained test runners that explicitly set
  `RIDGE_TEST_ALLOW_NON_BREAKAWAY=1` may use a test-only detached fallback.
  This fails closed instead of claiming force-kill survival without a real Job
  breakaway. Kernel boot now has a separate cross-process boot lock, preventing
  desktop/host startup races and duplicate kernel launches.
- Explicit full quit stops detached transports before kernel shutdown. A crash,
  force kill, or normal desktop restart never runs that cleanup path.

## Verification

| Gate | Result |
| --- | --- |
| `pnpm check` | 0 errors / 0 warnings |
| `pnpm exec vitest run src/lib/remote/totpIdentitySync.test.ts --reporter=dot` | 1 file / 1 test passed |
| `cargo test -p ridge-kernel --lib --quiet` | 46 passed |
| `cargo check -p ridge-cli --quiet` | passed; existing warnings only |
| `cargo test -p ridge-cli --tests --quiet` | 130 unit + 3 integration tests passed |
| `cargo check --manifest-path src-tauri/Cargo.toml --lib --quiet` | passed; existing warnings only |
| `git diff --check` | passed |

## Boundary evidence and remaining physical gate

The ownership graph is now process-level: `ridge-kernel` owns PTYs, `rdg host`
and `rdg remote --daemon` own transport, and Tauri owns UI/control commands.
The repository's detached process-tree regression test remains green. A final
physical Windows smoke (kill the packaged Tauri PID, then reconnect from a
phone and verify the same kernel PTY/output sequence) remains an external
release gate; no claim of that device evidence is made here.
