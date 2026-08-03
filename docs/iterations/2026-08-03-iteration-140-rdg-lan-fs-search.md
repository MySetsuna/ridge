# Iteration 140 - rdg LAN filesystem and search RPC closure

## Scope

Close the declared `pane/fs/search` capability gap in the headless `rdg` LAN
host. The host already advertised these capabilities, but its shared
`dispatch_lan_invoke` path silently rejected the filesystem and search methods.

## Change

- Reuse `ridge-cli::fs_reuse` for `search`, `get_directory_children`, and the
  legacy `search_files` shape used by the WebSocket data provider.
- Route `get_file_tree`, `read_file`, and `text_search` through the existing
  `ridge-core::dispatch` boundary.
- Derive serving roots once at the LAN host boundary and use the same roots for
  the trait-facing and JSON-RPC paths, preserving the filesystem sandbox.
- Add a deterministic JSON-RPC regression that creates a temporary tree under
  the serving root and verifies tree, children, canonical search, and legacy
  search response shapes.

## Verification

- `cargo test -p ridge-cli --bin rdg --quiet` - 127 passed, 0 failed.
- `cargo test -p ridge-cli --test kernel_lifecycle_e2e --quiet` - 3 passed,
  0 failed.
- `git diff --check` - passed.
- Commit `d5da7c2` contains the runtime fix; no version bump or release was
  made because `v0.1.54` consumed the publication allowance.

## Boundaries

This closes the LAN `fs/search` implementation gap only. Git/workspace
authority convergence, physical phone/public Remote evidence, WebView2 memory
soak, and dual-window/dual-host tests remain external or larger migration
items; they are not marked complete by this code-level guard.
