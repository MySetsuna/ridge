# Iteration 81 Contract — Agent Commune and Remote Mobile QoS

## Scope

- `REQ-AGENT-COMMUNE-UI-02` — approved Agent's Commune sidebar interaction and history continuity.
- `REQ-MOBILE-REMOTE-KEYBOARD-QOS-02` — approved stable IME geometry for the mobile Remote input.
- `REQ-REMOTE-RUNTIME-PERF-MEMORY-02` — approved Remote runtime, scrollback, and worker-resource reclamation.

Previous iteration debt was carried forward; no pending requirement remains for this intake. The Chrome `runtime.lastError` warning remains an attribution task: the repository contains no Chrome Extension Messaging API usage, so it is not masked in product code.

## NLM evidence and decision

- Recent project NotebookLM conversations reviewed: `68791fb7-659a-4ad6-a86c-beb7ac694781` and `a47d3199-c1f9-47f1-927c-ff2c4875b77d`.
- The latest kernel-first conversation was treated as strategic context only; stale TUI claims were rejected when they conflicted with the current frontend CodeGraph.
- Cold-loop response: `.iteration/nlm-iteration80-agent-commune-response.json`; schema gate passed.
- Decision package: `.iteration/decision-agent-commune-remote.json`; notebook gate allowed the cold loop.

Selected sequence: stable Agent/session identity → bounded all-CWD history projection → status/card view model → Pane status border projection. No new RPC protocol and no terminal-core rewrite.

## Implemented

- `src/lib/teammate/agentCommuneModel.ts`: deterministic identity normalization, cross-CWD grouping, status precedence, and labels.
- `src/lib/teammate/AgentCenterPanel.svelte`: all-CWD bounded history request, semantic card rail/status, and stale Pane-status cleanup; resume continues through `plan_agent_resume` and recorded `cwd`/`argv`.
- `src/lib/stores/paneTree.ts`: status projection store keyed by `workspaceId:paneId`.
- `src/lib/components/SplitContainer.svelte`: waiting/working/idle/stopped Pane outer-ring highlights and `data-agent-status`.
- `src/remote/lib/keyboardOffset.ts`, `src/remote/lib/TerminalCanvas.svelte`: bounded visual IME shift, hysteresis, finite settle, and close-to-zero reset without PTY/grid changes.
- `src/remote/lib/scrollbackWorker.ts`, `src/remote/MainApp.svelte`: bounded decode queue, timeout/worker-failure fallback, idempotent disposal, and teardown guards.

## Deterministic verification

- `pnpm exec vitest run src/lib/teammate/agentCommuneModel.test.ts src/remote/lib/keyboardOffset.test.ts src/remote/lib/scrollbackWorker.test.ts --reporter=dot` — 3 files, 19 passed.
- `pnpm check` — 0 errors, 0 warnings.
- Full suite: 118 files, 1369 passed, 1 skipped (1370 total); Vitest exit code 0.
- Requirements gate: executable intake, no pending IDs; iteration gate passes with pre-existing generated/user dirty baseline excluded from scope comparison.

## Release evidence

- `v0.1.24` release workflow `30706698658`: success, all five jobs.
- Remote publish `30706709638`: success.
- `ridge-cloud` deploy `30706722196`: success.
- GitHub Release `v0.1.24`: published `2026-08-01T16:43:05Z`, 11 assets covering Windows, Linux, and macOS.

## Closure status

Code slice is complete and pushed in focused commits. Final requirement closure remains gated on the full regression suite and physical/mobile UI evidence; until then these requirements stay Active rather than being falsely marked complete.
