# Iteration 156 - Kernel PTY restart reattach history

Date: 2026-08-04
Status: focused code green; full regression and physical restart evidence
remain open.

## Scope

Close the restart reattach gap in which the desktop restored a kernel PTY but
started its output lease at `next_seq - 1`. A restarted parser therefore saw
only future bytes, while unmatched kernel PTYs were silently ignored.

## Change

- Desktop restart reattach now uses `KernelPtyRef { after_seq: None }`, which
  asks the kernel output lease to replay its bounded retained window before
  live output. The kernel's existing replay cap remains the memory boundary.
- Reattach counts unmatched kernel PTYs and emits a structured warning with
  the count. Orphans remain alive for explicit recovery; the desktop never
  guesses that a missing layout means the user's terminal may be deleted.
- Existing pane-tree UUID matching and `install_kernel_pty` idempotency remain
  unchanged.

## Verification

- `cargo test -p ridge --lib pty_lifecycle_contract_tests`: 3 passed.
- Contract coverage asserts bounded replay and observable orphan accounting.

## Remaining gates

Run the full Rust/TypeScript matrix and a physical exit/restart test proving
the same PID, pane UUID, retained output, input, and resize survive. Health-aware
death watching, explicit orphan recovery UX, physical/public Remote, WebView2
heap soak, dual-window/Host, and complete Kernel domain authority remain open.
No version bump or release was made.
