# Iteration 142 - rdg legacy Remote sidebar frames

Date: 2026-08-03  
Status: code complete; public Remote and long-run performance evidence pending.

## Problem

The current rdg host already served filesystem and search through JSON-RPC, but
older Remote clients still sent the flat `list-files`, `list-git-status`, and
`search-files` frames. Those frames fell through a silent catch-all, so a legacy
sidebar could remain blank while the terminal stayed connected.

## Change

- `packages/ridge-cli/src/tui/lan_host_impl.rs`
  - resolve legacy paths against the active pane CWD and the same serving-root
    sandbox used by the canonical filesystem methods;
  - return the historical `files`, `git-status`, and `search-results` shapes;
  - run directory, Git, and search work in `spawn_blocking`, then enqueue the
    response on the existing WebSocket output channel so the WS event loop is
    never held by disk or Git work;
  - keep empty/error results fail-soft for rolling host upgrades.

## Verification

- `cargo test -p ridge-cli --bin rdg tui::lan_host_impl::tests --quiet`: 12
  passed, 0 failed (including asynchronous legacy file/search/Git frames).
- `rustfmt --edition 2021 --check packages/ridge-cli/src/tui/lan_host_impl.rs`:
  passed.
- `node scripts/rdg-remote-e2e.mjs --skip-build`: passed; isolated Chromium
  desktop `canvas=true tree=false ws=true`, mobile `canvas=true tree=true
  ws=true`, input/resize sent, and `browserErrors=[]`.
- Evidence: `.iteration/artifacts/rdg-remote-e2e/last-result.json` and the
  corresponding desktop/mobile screenshots.

## Not claimed

This closes the rdg legacy protocol gap only. Physical phone/public Remote,
WebView2 heap soak, dual-window/dual-host, and full Kernel-domain authority
migration remain external or larger-scope gates. No version bump or publication
was made because `v0.1.54` consumed today's allowance.
