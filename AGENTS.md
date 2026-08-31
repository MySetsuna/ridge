# Repository rules

SpecTree controls this repository. Do not edit authoritative specs by hand from
an agent session and do not bypass the gates.

1. Ask `stc next` for the next step; it reads the real state, not a memory of it.
2. `specs/` and `changes/` are authoritative. Proposals land in
   `.spectree/proposals/` and only reach specs through `stc apply --confirm`.
3. Code edits are authorized only for paths in the current lock allowed paths;
   `stc verify` rejects anything else.
4. Finish every round with `stc complete`, otherwise the next lock is blocked.
5. Removed modules, nodes, and documents are archived, never deleted.

## Architecture invariants

1. Core packages do not depend on Codex, Claude, Obsidian, or OpenSpec.
2. Portable Markdown/YAML sources compile into one normalized semantic graph.
3. LLM output may propose changes only; it never validates, approves, or verifies.
4. Graph, state, hashes, impact, locks, and trace checks are deterministic.
5. Code-to-spec inference produces evidence only and never edits authoritative specs.
6. Every behavior change maps to an authorized build unit.

## Iteration control

1. Every behavior change follows change, approve, lock, build, verify, complete.
2. Agent changes must land through a proposal and stc apply --confirm.
3. Code edits are limited to the current lock allowedPaths.
4. Do not create external iteration runtime or .iteration/ state.
5. Commit specs, changes, .spectree/config.json, approvals, and lock state when present.
6. Never commit tokens, cookies, secrets, or private keys.

## Engineering

Run npm test, npm run lint, and npm run typecheck before handoff.
