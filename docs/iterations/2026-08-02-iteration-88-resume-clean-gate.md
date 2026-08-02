# Iteration 88 continuation — Agent resume and release cleanliness

Date: 2026-08-02
Baseline: formal desktop release `v0.1.40` (`c0d7bce`)

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

## Publication result

The gate passed. Commit `c0d7bce` aligned all four version sources at `0.1.40`,
was pushed before tagging, and produced annotated tag `v0.1.40`. Release run
`30752429063` passed the test gate and all four build jobs; the formal release
is non-draft/non-prerelease with 12 matching installer/CLI assets. Remote run
`30752469369` passed and atomically activated `0.1.40+gc0d7bce`; Cloud health
returned HTTP 200. No failed versioned release or orphan version bump remains.

## External gates retained

Physical phone `runtime.lastError` attribution, physical iOS/Android/PWA
safe-area/keyboard/touch, public WebRTC, authenticated Git push, WebView2
long-run heap, dual-window/Host device singleton E2E, and full desktop
AppState/PTY/filesystem-root Kernel authority migration still need their
respective runtime evidence. Local tests do not overclaim those gates.

## Continuation slice: Remote Query lifecycle and Agent Commune

- Sidebar cache keys now include remote session, workspace, pane, CWD/path, and
  optional branch. Git/file mutations invalidate only the matching scoped
  prefix, preventing same-path cross-pane/workspace cache reuse.
- Pane/workspace snapshots clean listeners on success, abort, send failure, and
  bounded timeout (`15s` default). `WsDataProvider` now propagates AbortSignal
  through sidebar reads and rejects all data requests on transport
  disconnect/error or synchronous send failure; `dispose()` detaches listeners
  and clears timers.
- Desktop Agent history resume is single-flight, all live Agent rows expose a
  status-colored rail, and Remote history scans run on a strict five-minute
  cadence rather than every third roster poll.

Evidence: `pnpm exec vitest run src/remote/lib/remoteQueries.test.ts
src/remote/lib/sidebarProvider.test.ts src/lib/teammate/agentCommuneModel.test.ts
src/lib/transport/ws.test.ts` — 4 files, 20 tests passed; `pnpm check` — 0
errors, 0 warnings. `pnpm e2e:rdg-lan -- --skip-build` passed desktop/mobile
LAN checks (`ws=true`, `browserErrors=[]`, input/resize true); this is LAN
evidence only.

Residuals remain explicit: Tauri host calls do not interrupt an already-started
IPC operation; PaneInfo has no canonical Git branch;
desktop history lacks OpenCode/MiMo/Chinese CLI adapters and stable native
session merge; Remote resume CWD is directory-checked but not canonical-
session-bound; public/physical Remote, WebView2 heap soak, authenticated Git
push, dual-window/Host, and full Kernel domain authority migration remain open.

## Final publication for the continuation slice (2026-08-03)

The landed Query/Agent changes were committed as
`22e6e2933d9adfd8d413134052eff9cbac17e1d9`, versioned `0.1.41`, and pushed
before tagging. Release workflow `30755076173` passed its test gate and all
four platform build jobs. GitHub Release `v0.1.41` is non-draft/non-prerelease
with all 12 matching installer and `rdg` CLI assets. Remote workflow
`30755719992` succeeded from the same commit. Cloud health returned HTTP 200
(`version=0.0.7`).

The release gate was rechecked after publication: tracked and staged diffs are
empty, no untracked non-ignored files remain, and `HEAD == origin/main`.
Release publication includes the modified code; no failed version bump or
orphan tag remains.

## Post-publication Agent resume guard (2026-08-03)

Remote `resume_agent_session` now resolves the host-owned history row by the
exact `(agent, sessionId)` pair, uses its recorded CWD, canonicalizes both
paths, and rejects a client CWD mismatch before creating a PTY. The lookup is
unbounded for authority purposes; the 100-row cap remains a presentation cap.
This closes the prior directory-only validation gap without trusting mobile
display state.

Evidence: `cargo test --manifest-path src-tauri/Cargo.toml
commands::project --lib --quiet` — 25 passed; `cargo test
--manifest-path src-tauri/Cargo.toml commands::pane --lib --quiet` — 13
passed; `pnpm check` — 0 errors, 0 warnings.
