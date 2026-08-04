# Iteration 165 — Tab transition serialization and kernel deep-root continuity

## Scope

Close the remaining verified regressions after iteration 164: rapid desktop
tab clicks could still send backend workspace switches concurrently, and the
tray's desktop-exit path stopped the process (and therefore Remote) even though
the kernel was intended to survive. Preserve active PTYs and Agent negotiation
across a desktop process restart where the kernel endpoint remains alive.

## Change

- Add a single FIFO workspace-switch queue in `src/lib/stores/paneTree.ts`.
  Cached tabs still activate immediately, but `claim_workspace_window`, switch,
  and layout IPC cannot overtake an in-flight transition. The existing
  latest-generation guard suppresses stale layout/error publication.
- Change tray `退出桌面端` to Deep Root UI-only hide. It emits the existing
  memory-reclaim event and keeps the Tauri host, kernel, PTY, Remote server, and
  teammate server alive; only `彻底退出（将一并退出 rdg）` shuts them down.
  Save the restore set at the hide boundary as well as the true quit boundary.
- Persist the teammate loopback `{base_url, token}` in app data with a
  write-then-replace file path. On startup, reuse the preferred port/token when
  available; on a port collision, use an ephemeral port and refresh matching
  token sidecars, allowing existing tmux/Agent processes to rediscover the new
  URL without changing their kernel-owned PTY.

## Verification

- `pnpm exec vitest run src/lib/stores/paneTree.test.ts`: 65 passed, including
  an in-flight switch ordering regression test.
- `pnpm test`: 148 files, 1545 passed, 1 skipped. The renderer contract test
  was updated to assert the current serialized memory-restore queue rather
  than a removed `Promise.all(pending)` implementation string.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib teammate::endpoint::tests`:
  3 passed (binding validation and restart sidecar refresh).
- `cargo check --manifest-path src-tauri/Cargo.toml`: passed; existing warning
  set only.
- `pnpm check`: passed, 0 errors / 0 warnings.
- `git diff --check`: passed.
- Release `v0.1.56`: GitHub Actions test gate and all four platform jobs passed;
  the published Release contains 13 installer/CLI assets. Remote bundle
  `0.1.56+g64817b9` dry-run produced 268 files (24.83 MiB). Cloud upload was
  not attempted beyond the credential check because
  `RIDGE_CLOUD_ARTIFACT_URL` / `RIDGE_ARTIFACT_TOKEN` are absent locally.

## Boundary / external evidence

Deep Root v1 deliberately hides rather than destroys the Tauri WebView because
the LAN/cloud Remote server and WebRTC provider still live in that host process.
Thus Remote continuity is verified for the supported UI-only exit path; a hard
kill of the desktop process would require moving that server/provider into the
kernel host and is not claimed by this iteration. Physical restart, public
Remote, WebView2 heap soak, and dual-window visual evidence remain external
gates.
