# App-server terminal attachment — 2026-10-16

Implemented the app-server wire driver alongside ACP. Both use the same Runtime
member registry, fenced delivery queue, permission service and durable protocol
history. Production attachment never launches a process or creates a thread.

Real Codex CLI 0.162.1 acceptance:
`D:/ridge-070-acceptance/app-server-terminal-kgPaRM/result.json`.
The owned fixture starts an app-server and its terminal TUI under one Runtime
PTY, with the TUI explicitly resuming the same thread. An isolated local model
fixture supplies deterministic responses; no personal Codex profile or external
model account is used. The test proves:

- An opted-in catalog discovers the running party's exact process descriptor and
  automatically attaches the existing loaded thread.
- Pre-attachment history comes from `thread/turns/list`, without private vendor
  history parsing. The terminal displays that existing history before attachment.
- A sidebar delivery addresses that same thread; its reply appears in the same
  terminal TUI and once in Commune history. The original PTY process is retained.
- `turn/start` is a receipt; only the matching `turn/completed` settles delivery.
  History reads do not invoke a model or resume a thread.

The protocol unit tests additionally reject unloaded threads without resuming or
creating them, fence thread/turn completion, reject overlapping local sends, and
cancel through `turn/interrupt` with the confirmed turn id. WebSocket credentials
are file references, bounded and never persisted in the member/history DTO.

Catalog connection metadata (existing Agent JSON editor):

```json
{
  "connection": {
    "protocol": "app-server",
    "descriptorFile": "C:/absolute/run-directory/connection-{pid}.json",
    "autoConnect": true
  }
}
```

The running party supplies a descriptor with `protocol`, `process`,
`endpointOwner`, loopback `endpoint`, exact `sessionId` (app-server thread id),
and absolute `cwd`; `bearerTokenFile` is optional. Process objects contain `pid`
and `startInstance`. Runtime validates the actual pane process tree, OS socket
ownership, protocol identity and loaded thread before publishing membership.
Manual trusted registration remains available. Explicit removal prevents the
supervisor from automatically reattaching that retired process instance.

## Ordinary terminal interaction

This receipt covers shared app-server terminal mode, as documented in the
[official CLI reference](https://learn.chatgpt.com/docs/cli/reference) and
[app-server documentation](https://learn.chatgpt.com/docs/app-server).
Protocol attachment requires an endpoint. Ordinary terminal interaction now uses
the approved human input bridge and existing Runtime parser; no changed startup
command, shared app-server or descriptor is required. The standalone real Codex
receipt is `D:/ridge-070-acceptance/app-server-terminal-pvbCSi/result.json`.
The local provider receives a sidebar prompt and its reply appears in the same
original PTY. Reads do not call the model; exact input retries are deduplicated.
Paste and Enter are separate acknowledged writes so a TUI paste burst cannot
swallow submission. Unknown acknowledgments retain only the pending phase.

The desktop receipt is
`D:/ridge-070-acceptance/temp/ridge-terminal-ui-PpaQM7/result.json`: two workspaces
appear together; text and selection keys reach their original PTYs; repeated
input executes once; the actual pane fills its grid and follows shrink/growth.
Runtime integration also rejects stale terminal lifetimes, reused process
instances, cross-workspace views and untrusted manual input.

The user’s running CLI, global shell/profile and installed application remain
untouched. Historical sessions not supplied by an attached protocol source remain
outside protocol history. Terminal screen projection does not invent those records.
