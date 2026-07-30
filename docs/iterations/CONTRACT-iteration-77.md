# Iteration 77 Contract — three-day residual audit and external-evidence closure

- Date: 2026-07-30
- Status: audit in progress
- Baseline: `a81674a` (local `main`, no push/release)
- Requirements: `REQ-20260730-01`, `REQ-MOBILE-REMOTE-RUNTIME-LASTERROR-01`, and the active requirements carried by iterations 67–76
- Authority: current code/tests first; NotebookLM is strategy only; physical and production claims require captured evidence

## Three-source reconciliation

| Area | Latest code fact | Classification | Required closure |
| --- | --- | --- | --- |
| RPC, input/resize, SCM negative cache, pane destroy, error aggregation | `5eece08`–`826e3a1`, `9904b53`, `fc6a73b`, `a01f7db`, `20d7be3`, `2052753`; quantified gate 70/70 | code complete | retain regression and collect public A/B |
| Scrollback cap, pressure/teardown, right-click/clear | `c1ec8a2`; ridge-term 395 + protocol smoke 33 | core complete | WebView2 long-run memory/GC plateau |
| Host discovery, progress, drag, measured resize | `7b7daee`, `c290143`, `0d273c3`; host gate green | code complete | dual-host LAN/public list/create/drag/resize run |
| Multi-window workspace ownership and window-local active state | `5723828`, `a9023f3`; Rust/frontend ownership tests green | code complete | physical two-window focus/race/close run |
| Commune visibility and MCP submit | `2b53650`, `dfa3d2e`, `80ac077`; focused tests green | code complete | visible Agent/Commune E2E and real recipient acknowledgement |
| Mobile composite identity, Worker, keyboard, background continuity | `c73ce87`, `d2b8b82`; deterministic suites green | code complete, physical proof missing | iOS Safari/Android Chrome weak-network and lifecycle run |
| Public Remote geometry/WebRTC | LAN/fixture geometry exists; production controller/TURN not measured | external proof missing | capture DOM rect, DPR, kernel rows×cols, resize and reconnect |
| WebView2 memory | bounded implementation exists; no long-run curve | external proof missing | fixed scenario A/B with heap/private bytes and scrollback counters |
| Mobile `runtime.lastError` | no project Extension Messaging APIs; only Service Worker `Client.postMessage` | source excluded, attribution pending | clean profile/incognito and injector A/B on affected phone |
| Agent history adapters | Claude/Codex structured resume implemented; Grok explicitly disabled without verified native format | intentional capability gap | do not guess Grok schema; add adapter only with native fixture |
| Native headless sessions | Ridge-owned projection and summon path implemented; real process chain not run | external proof missing | create/list/summon/detach one Ridge-owned session |
| Explorer and terminal raster | deterministic semantics/geometry green; cross-volume/permission and native PowerShell/PTY traces absent | external proof missing | run Windows volume matrix and approved DPR/backend recordings |

## Execution order

1. Run deterministic local regression gates for every row marked code complete.
2. Refresh the project state and NotebookLM source snapshot with the live-auth fact and this matrix; do not add a third Notebook source.
3. Execute available isolated Dev/CDP checks without touching the installed Ridge host. Preserve failed external runs as explicit evidence gaps.
4. User-track closure order: mobile attribution → public Remote/dual Host → WebView2 memory → two-window desktop → Agent/Headless → Explorer/raster recordings.
5. If any run shows a code defect, open one focused fix slice, add a deterministic reproducer, then rerun dependent rows. If only a device/credential is missing, do not invent a code change.

## Live NLM cross-check (strategy, not implementation evidence)

The authenticated query over the current two Notebook sources confirmed the same
residual ordering: mobile continuity and public geometry first; then real Agent
CLI/history, Windows Explorer behavior, terminal raster/PTY traces, and
Ridge-owned headless session evidence. It also warned that LAN fixtures cannot
stand in for iOS/Android, public WebRTC/TURN, WebView2 long-run, or native CLI
process evidence. These statements are recorded as audit priorities only; every
row above remains governed by local symbols and deterministic/physical evidence.

## Acceptance and regression

- Local gate: requirements gate, strict preflight (untracked runtime dirs allowed), focused Vitest/Rust suites, `pnpm check`, Remote desktop/mobile build, and `git diff --check` all report exit `0`.
- 2026-07-30 local run: focused Vitest `12 files / 253 tests` passed; `pnpm check` reported `0 errors / 0 warnings`; `cargo test -p ridge-term --lib` reported `395 passed`; remote smoke evidence example validator passed; `pnpm build:remote` exited `0` (140.4 s). Build output retains non-blocking dynamic-import/chunk-size and empty PWA glob warnings; no new owned browser warning was introduced.
- No claim of “overall complete” until the external rows contain timestamped evidence with browser/OS/build and scenario details.
- `runtime.lastError` acceptance is zero business-code diff unless a first warning source URL proves project ownership.
- Grok remains `canResume=false` until an actual native history fixture proves executable/argv/cwd/session semantics.
- No host Ridge launch/termination, no release, no push, and no deletion of user data in this iteration.
