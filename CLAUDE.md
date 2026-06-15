# Project Guidelines

## Communication
- Think in English, respond in Simplified Chinese. Code comments follow the existing language style of each repo.

## Workflow
- Use the Superpowers skills for non-trivial work: `brainstorming` → `writing-plans` → `executing-plans` (or `subagent-driven-development`). Process skills (brainstorming/debugging) come before implementation skills.
- Write design specs and plans under `docs/superpowers/specs/` (filename `YYYY-MM-DD-<topic>-design.md`); commit the design doc before implementing.
- **If there is even a 1% chance a skill applies to the current task, invoke it.** Don't skip skills because the task seems simple.

## Collaboration Rules
- Follow the user's instructions precisely, and within that scope act autonomously: gather the necessary context and complete the requested work end-to-end in this run, asking questions only when essential information is missing or the instructions are critically ambiguous.
- For hard-to-reverse or outward-facing actions (deletes, pushes, publishing), confirm the scope first unless explicitly authorized.
- One feature/concern per commit; keep commits focused.

## Code Intelligence
- See `.claude/CLAUDE.md` for the CodeGraph MCP usage guide — prefer it for structural code questions (callers/callees/impact/definitions) over grep.
