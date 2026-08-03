# Iteration 153 - Kernel-owned desktop Git status

Date: 2026-08-04
Status: code green; physical and cross-host gates remain open.

## Scope

Close the desktop SCM read-path gap identified after PTY authority converged.
The desktop Git status command must use the authenticated kernel Git domain so
SCM polling shares one bounded external-process boundary and the kernel can
apply repository detection and process-lifecycle policy consistently.

## Change

- `get_scm_status` now obtains the singleton kernel endpoint and reads
  `/v1/domain/git/status` instead of invoking the local `ridge-core` Git
  implementation directly.
- Kernel responses are source-checked (`source = ridge-kernel`), distinguish a
  confirmed non-Git path from transport/domain failure, and decode the shared
  `ScmRepoStatus` shape without changing Query/slot call signatures.
- Git paths are URL encoded before the domain request; malformed, stale, or
  non-kernel responses fail closed.
- `ScmFile` and `ScmRepoStatus` now derive `Deserialize` for the typed adapter.

## Verification

- `cargo test -p ridge-kernel --lib client::tests::domain_git_status`: 2 passed.
- `cargo check -p ridge --lib`: exit 0 (pre-existing warnings only).
- The new client tests cover non-Git detection, status fields, source rejection,
  and Windows-style query escaping.

## Remaining gates

Full Rust/TypeScript regression, real desktop SCM polling against Git and
non-Git roots, physical/public Remote, WebView2 heap soak, dual-window/Host,
and complete Kernel domain authority evidence remain open. This slice does not
claim Git branch/list mutation convergence or external release readiness. No
version bump or release was made.
