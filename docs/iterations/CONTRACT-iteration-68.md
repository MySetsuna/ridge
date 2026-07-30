# Iteration 68 Contract — Agent interaction and session history

- Date: 2026-07-29
- Status: approved / implementation
- Requirements:
  - `REQ-AGENT-INTERACTION-STATE-01`
  - `REQ-AGENT-HISTORY-SOURCE-02`
  - `REQ-AGENT-HISTORY-01`

## Delivery boundary

1. Agent navigation and attention
   - Clicking a live member switches by explicit workspace id, selects its pane,
     focuses the actual terminal input, then acknowledges attention.
   - Waiting approval is amber; stopped/disappeared is red.
   - Agent row and pane-header icon read one transient attention store.
   - Text and aria labels carry the same semantics; color is not the sole signal.
   - Unchanged topology polls do not re-create an acknowledged highlight.

2. Initial Agent panel width
   - Opening Agent's Commune expands a collapsed or undersized sidebar to the
     normal minimum without requiring a second tab switch.

3. Native history
   - Backend emits one row per native session id, with stable title, id, agent,
     cwd, latest activity, latest assistant output, and structured resume spec.
   - Duplicate messages from one session collapse to its latest assistant item.
   - A running native session is matched only by exact native session id.
   - Resume creates one new pane and launches `executable + argv + cwd` through
     the existing structured PTY path; no shell command concatenation.
   - Claude and Codex remain enabled. Grok stays honestly unavailable until a
     verified local/native format exists.

## Non-goals

- No title/cwd heuristic identity merge.
- No upload or mutation of third-party history.
- No arbitrary external PID takeover.
- No new polling loop.

## Deterministic gates

- teammate model tests
- Rust parser/session aggregation tests
- touched Svelte diagnostics
- focused terminal command compile/tests

