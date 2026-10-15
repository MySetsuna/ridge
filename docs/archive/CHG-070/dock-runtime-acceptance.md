# Runtime dock acceptance

Migration evidence for `dock_pane`; not a CHG-070 completion certificate.

The common Runtime command supports all five existing dock regions. Cross-workspace moves preserve pane metadata, locked sizes, foreign bindings and the original local PTY. A source workspace emptied by docking is archived and retired; clients selecting it fall back to the destination. Other clients retain their own valid selection. Registered protocol members must remove membership before a cross-workspace move; their workspace, generation and lease are never silently rebound.

Graph publication and local PTY retargeting share the terminal input fence. A failed graph write leaves the live ownership unchanged. The old workspace cannot write, resize, snapshot, page history or subscribe after a move. Associated `.ridge` files use the existing Runtime autosave worker. Queue admission failure after graph commit reports that the dock committed and does not replay it.

Passing targeted checks:

- `ridge-core`: `dock_between_preserves_metadata_and_commits_only_valid_moves` covers five regions, atomic rejection, metadata/foreign binding preservation and empty-source retirement.
- `dock_runtime.rs`: real shell children, failed publication, scoped file rejection, original PID/start instance/terminal/history epochs, bounded history, input deduplication, stale workspace rejection, old-stream closure, destination input, associated file autosave, archive and two-client selection isolation.
- `commune_registration.rs`: the existing real protocol-process fixture rejects moving its registered member across workspaces and continues the original conversation/permission/execution checks.
- `outbound_hosts_runtime.rs`: the existing two-Runtime authenticated TLS fixture moves a foreign view out and back, closes the old local stream, rejects its old scope, and retains the original remote terminal epoch/process.

Logs: `.webview2-dev-cdp/070-dock-runtime.log`, `070-dock-affected.log` (the final Core result), and `070-dock-affected-final.log`. Earlier failed fixture attempts remain visible in the logs; their corrected reruns above pass.

Handoff gates: npm test (2,137 passed, 14 skipped, plus six Node tests), lint and typecheck (zero errors/warnings), and `stc verify` (VALID). This round ran affected Rust checks and the required handoff gates once after fixes; it did not repeat the prior performance matrix. Prior performance receipts predate this input-fence change, so no new latency claim follows from them.

Still outside this receipt: prompt/PTY-close/terminal-error event migration; user-started external Agent desktop-kill acceptance; actual installed release/upgrade lifecycle; the remaining migration failure matrix; controller identity approval. The pending identity proposal remains unapplied.
