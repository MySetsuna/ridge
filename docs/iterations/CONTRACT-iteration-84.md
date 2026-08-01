# Iteration 84 Contract — Agent History Source Fairness

## Scope

Close the approved Agent's Commune history gap without changing the card or
resume protocol: every supported local history source remains discoverable
when one source has a busy session tree, and each reply keeps its recorded CWD
for display and structured resume.

## Root cause

`read_agent_recent_replies_sync` appended Claude and Codex JSONL paths into one
bounded vector. `collect_jsonl_files` stopped at the shared cap, so a Claude
tree with enough files could prevent Codex discovery entirely. The frontend
already grouped replies by stable Agent identity, but it could not group a
source that the backend never returned.

## Implemented

- Run the bounded JSONL discovery independently for Claude and Codex, then
  sort the combined metadata by modification time and retain the global latest
  200 files before parsing.
- Add a filesystem fixture covering Claude and Codex in different nested
  directories, different CWDs, latest assistant text, structured resume
  arguments, and a child-path filter.
- Preserve existing malformed-line tolerance, per-session latest-row
  deduplication, and bounded file-window reads.

## Verification

- `cargo test --manifest-path src-tauri/Cargo.toml --lib project::tests`: 22
  passed.
- `pnpm exec vitest run src/lib/teammate/agentCommuneModel.test.ts`: 3 passed.
- `cargo check --manifest-path src-tauri/Cargo.toml --lib`: exit 0; 39 existing
  warnings, no new error.
- Commit `b88b679` pushed to `origin/main`.
- Version contract `0.1.30` committed as `58c2cb7`; release, Remote artifact,
  and ridge-cloud runs were dispatched from those source revisions. Their
  completion and asset/health evidence must be appended after all jobs finish.

## Non-claims

This closes deterministic source-discovery coverage only. It does not claim a
real Claude/Codex/Grok process session, physical phone, public Remote,
WebView2 heap soak, or dual-window/dual-host evidence.

## Closure status

Code and deterministic tests are complete. Iteration release/deploy evidence
and the previously listed external gates remain tracked separately.
