# Iteration 137 — Agent attention and live Pane titles

## Scope

Close the Agent Commune visual state loop without introducing a second polling
source. Agent cards and Pane chrome must consume the existing workspace/pane
identity and the same live title stores used by `PaneHeader`.

## Changes

- `AgentCenterPanel` projects `terminalTitles`, foreground process, and
  workspace-scoped CWD into each card title. Agent identity remains stable for
  commands, history, and resume actions.
- A working-to-idle transition emits an `idle` attention event. Initial idle is
  neutral, so opening Commune does not flash every already-idle Agent.
- Waiting approval and stopped events remain persistent until the target Pane
  is focused. A newer waiting/stopped event may upgrade an unacknowledged idle
  event, but no event is downgraded or silently acknowledged by polling.
- The Pane border, Pane Agent button, and Agent card share the attention value;
  `RidgePane`/card activation remains the sole clear path.
- Pure transition and priority helpers plus source-contract tests guard the
  behavior.

## Verification

- `pnpm check` — 0 errors, 0 warnings.
- `pnpm exec vitest run src/lib/teammate/agentCommuneModel.test.ts src/lib/components/SplitContainer.test.ts --reporter=dot` — 2 files, 12 tests passed.
- `git diff --check` — expected to pass before commit.
- No version bump, release, Remote cloud publish, or public deployment; the
  daily publication allowance remains consumed by `v0.1.54`.

## Acceptance and remaining evidence

Code acceptance covers live title projection, working-to-idle detection,
approval/stopped persistence, priority upgrades, and focus acknowledgement.
Physical desktop/phone rendering, long-running memory soak, public Remote, and
full Kernel-authority evidence remain external gates and are not claimed here.
