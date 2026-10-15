# Runtime terminal event acceptance

Migration evidence, not a CHG-070 completion certificate.

`pane-prompt-{workspaceId}-{paneId}` now originates in the existing Runtime VT parser. OSC 133/633 prompt-start signals drain through a bounded transient flag; no second parser, raw-output forwarder or terminal wire field was added. Malformed `AX` is not a prompt. The registry checks the original terminal epoch and current workspace before publishing, so a dock changes the notification's workspace without replacing the child. The reader starts after registry installation, avoiding a lost first prompt.

`pane-terminal-error` now originates in the common Host subscription task. Connect timeout, malformed binary frames and an unexpected stream close are visible to that connection's current activation. Dock, pane removal, unsubscribe and a replacement subscription do not turn an obsolete task into a current failure. An internal lease fences task cleanup independently of the wire activation; same-activation resync opens a fresh snapshot stream, while a lower activation cannot displace the current one. The private connection handshake uses the existing five-second bound.

Actual execution checks:

- `ridge-term` test `prompt_signal_uses_streaming_parser_and_drains_without_reset_false_positives`: fragmented OSC/BEL/ST, malformed marker, drain-once and reset behavior.
- `terminal_events_runtime.rs` test `runtime_prompt_tracks_dock_and_terminal_failure_is_connection_and_activation_scoped`: an owned PTY child writes real OSC markers; two Native-grant WebSockets receive the current workspace/terminal epoch; docking retains the process; resync retains the epoch; lower activation and old-task cleanup cannot remove the replacement; real PTY removal fails only the current subscribed activation, with no error broadcast to the observer.
- The ignored `terminal_event_child_fixture` is executed as that real child, not counted as an independently passing test or an external Agent.
- Existing authenticated TLS/foreign-view regression: `authenticated_tls_peer_is_kernel_owned_and_never_fabricates_probe_sessions` in `outbound_hosts_runtime.rs`.
- Existing Cloud facade regression: `cloud_transport_reuses_host_domain_state_and_seals_remote_role` in `remote_invoke_runtime.rs`, including stale bind/unbind, shared terminal input and Remote role/root boundaries.

Rust logs: `.webview2-dev-cdp/070-terminal-prompt-parser.log`, `070-terminal-events-runtime.log`, `070-terminal-events-foreign.log`, `070-terminal-events-remote-invoke.log`. Handoff logs: `070-terminal-events-final-test.log`, `070-terminal-events-final-lint.log`, `070-terminal-events-final-typecheck.log`, `070-terminal-events-final-stc-verify.log`. Earlier fixture corrections do not stand in for the final passing reruns.

The real event fixture proves Native-grant delivery and the shared Host path. It is not a rendered GUI, WAN, packaged release or external user-Agent acceptance receipt. No performance matrix was repeated and no new latency result is claimed.

Still open: `pane-pty-closed` remains disconnected from Runtime business events. Its frontend handler currently chooses and recreates a shell. That decision must move to Runtime together with exit/rebuild notification and a policy that cannot restart a user Agent or produce duplicate children. Legacy global layout/focus/activity consumers, installation/upgrade, the remaining migration failure matrix, external user-Agent acceptance and controller identity approval are separate unfinished work.
