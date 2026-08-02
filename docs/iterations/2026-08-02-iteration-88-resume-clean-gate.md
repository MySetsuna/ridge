# Iteration 88 continuation — Agent resume and release cleanliness

Date: 2026-08-02
Baseline: formal desktop release `v0.1.39` (`c772085`)

## Landed

- `86f24af` fixes workspace-qualified Remote pane kernel detachment. Closing a
  pane now reaches the matching scrollback/kernel/worker owner; equal pane IDs
  in different workspaces do not alias.
- `a908066` adds the optional `RIDGE_KERNEL_FS_ROOT` fail-closed boundary and
  preserves the no-Tauri kernel host smoke contract. Full desktop authority
  migration remains explicitly open.
- `b8ac246` completes Agent history resume on Remote. The card shows the
  recorded CWD; `resume_agent_session` is admitted by the transport and Host;
  Host validates the directory, resolves the registered profile, launches a
  structured argv PTY, activates the new pane, and emits topology events.
  Client input never becomes a shell command string.
- `19b20c2` stabilizes lifecycle probes after reconnect; `d0d1ab6` adds the
  release worktree cleanliness rule and ignores only verified generated,
  runtime, and reference checkout paths.

## Evidence

- `pnpm exec vitest run src/remote/lib/SidebarTeamRoster.test.ts src/remote/lib/cloudRemote.test.ts packages/remote/src/shared/cloud/workspaceScope.test.ts src/remote/lib/paneLifecycle.test.ts` — 4 files, 46 tests passed.
- `pnpm check` — 0 errors, 0 warnings.
- `cargo test --manifest-path src-tauri/Cargo.toml commands::pane --lib --quiet` — 12 passed.
- `cargo test -p ridge-kernel --lib --quiet` — 23 passed.
- `scripts/kernel-host-smoke.ps1` — `ALL SMOKE PASSED`, including root in/out
  checks, Agent/Git/Host/MCP topology, and process cleanup.

## Release gate

No version bump, tag, GitHub Release, Remote artifact, or Cloud publication is
valid while useful code is uncommitted/untracked. Before the next release:

1. classify every dirty path; land real logic and tests, ignore generated/runtime
   output, or remove obsolete material safely;
2. commit and push all landed code;
3. require empty `git diff`, empty `git diff --cached`, empty
   `git ls-files --others --exclude-standard`, and `HEAD == origin/main`;
4. only then bump to the next version and publish. If CI/release fails, do not
   advance or leave a new version behind.

## External gates retained

Physical phone `runtime.lastError` attribution, physical iOS/Android/PWA
safe-area/keyboard/touch, public WebRTC, authenticated Git push, WebView2
long-run heap, dual-window/Host device singleton E2E, and full desktop
AppState/PTY/filesystem-root Kernel authority migration still need their
respective runtime evidence. Local tests do not overclaim those gates.
