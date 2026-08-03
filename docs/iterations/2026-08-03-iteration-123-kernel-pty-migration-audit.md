# Iteration 123 - Kernel PTY migration boundary audit

Date: 2026-08-03  
Status: audit complete; no unsafe production migration made

## Finding

The Tauri shell still owns the live desktop PTY lifecycle. `write_to_pty`,
`resize_pane`, clear, kill, and scrollback use the `(workspace_id, pane_id)`
AppState maps, native PTY handles, parser, and local scrollback. The kernel
contains a separate PTY registry and domain endpoints, but it does not yet
provide the bridge contract required for a safe handoff:

- stable composite pane identity shared by shell and kernel;
- PTY lease/attach/detach ownership transitions;
- live output stream and backpressure delivery to the shell/Remote clients;
- resize/clear acknowledgements and cancellation tied to pane destruction;
- persistence/recovery semantics for the existing desktop workspace projection.

Redirecting one Tauri command to the current kernel endpoint would therefore
create a second PTY, lose live output, or split lifecycle state. That is not a
valid convergence step and was deliberately not implemented.

## Evidence

- `src-tauri/src/commands/terminal.rs:1143-1324` — local input path;
- `src-tauri/src/commands/terminal.rs:1338-1658` — local/native resize path;
- `src-tauri/src/commands/terminal.rs:1670-1707` — parser and AppState clear;
- `src-tauri/src/commands/terminal.rs:1797-1865` — local PTY teardown;
- `packages/ridge-kernel/src/domain.rs:542-650` — isolated kernel PTY API;
- `src-tauri/src/kernel_lifecycle.rs:66-110` — kernel discovery/spawn only;
- `src-tauri/src/lib.rs:332` — deep-root migration remains incomplete.

## Next safe order

1. Define a versioned kernel PTY lease and `(workspace_id, pane_id)` mapping.
2. Add one bounded output stream with sequence, backpressure, cancellation, and
   destroy ACK semantics.
3. Add a shell adapter and deterministic dual-owner/lifecycle tests.
4. Migrate one command family only after the adapter proves no duplicate PTY,
   zero pending requests after destroy, and output continuity across reconnect.

No version bump or publication was made because `v0.1.54` consumed today's
release allowance.
