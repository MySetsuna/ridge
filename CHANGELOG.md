# Changelog

All notable changes to **Ridge** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.1.0] — 2026-04-30 · 「开荒 / Breaking Ground」

The first public release of Ridge. The plot lines are laid; from here on every
furrow is cultivation.

### Terminal & PTY
- portable-pty + xterm.js (WebGL renderer) terminal with Unicode 11, hyperlinks,
  and addon-search wired up.
- Block-paged scrollback: 64 KiB blocks, 4 MiB total cap, deterministic seq-byte
  paging via `get_pane_scrollback_tail` / `get_pane_scrollback_before`.
- Pane title bar shows foreground process + cwd (multi-coloured), with IME
  composition guard.
- OSC 7 cwd sync; Windows paths force-normalised to forward slashes across
  frontend and backend.
- `ScrollbackHistoryModal`: 256 KiB initial pull + 128 KiB "load older" paging,
  ANSI stripped for clean copy/search, in-modal search with n/N navigation.

### Split panes & workspaces
- Recursive pane tree (horizontal / vertical / nested) with keyboard shortcuts
  and drag-resize.
- Multi-workspace support, independent PTY namespaces, inactive workspaces stay
  alive in the background.
- Two-level Explorer tree (workspace → terminal), drag/rename/keyboard nav.
- Collapsible sidebar with shortcut.

### Editor
- Monaco editor as a second pane mode, sharing the same PaneTree as terminals.
- Editor font dropdown; three themes shipped.
- Sidebar Search: parallel scan of every distinct cwd in `paneCwdStore` with
  dedupe, glob filters, optional replace bucketed per repo root.

### Git / SCM
- Canvas-rendered Git Graph.
- `PaneGitPill` per pane: branch + diff summary + inline branch picker /
  "+ create new branch" input.
- SCM status driven entirely by `.git/` file watching (no polling). Linked
  worktrees auto-detected by parsing the `.git` redirect file.
- Round-64 SCM refresh policy: only two trigger paths (cwd change, watcher
  event); mount-time discover is skipped if the cache is fresh (<30 s).

### Agent teams
- Self-built tmux-compatible shim binary (`src-tauri/src/bin/tmux.rs` →
  `dist/teammate-shim/tmux.exe`), bundled into the installer with PATH
  registration via NSIS / WiX.
- Shim translates `list-panes` (with cwd) / `display-message` template vars /
  `kill-pane` / `rename-window` into POST/GET against the local teammate API.
- `RIDGE_TEAMMATE_URL` / `RIDGE_TEAMMATE_TOKEN` injected into PTY child env so
  Claude Code agents inherit them automatically.

### UI / DX
- `WindDialog` API replaces all native `alert` / `confirm` / `prompt` to keep
  the chrome consistent inside Tauri.
- overlayscrollbars takes over complex scroll regions; horizontal-tabs preset
  for tab strips.
- Modal z-index registry — every overlay mounted as a sibling of the root
  layout to avoid parent stacking-context bleed.
- `ContextMenu` keyboard navigation (Up/Down/Home/End/Enter/Esc/Right/Left).

### Engineering gates
- `cargo build --lib` emits **0 warnings** as of round 19. CI can safely run
  `cargo clippy -- -D warnings`.
- Future-use APIs and legacy compatibility stubs tagged with `#[allow(dead_code)]`
  + a one-line comment explaining why.

### Docs site
- GitHub Pages site under `/site/` with theme-matched homepage, docs hub, and
  release log; deployed via `.github/workflows/deploy-pages.yml`.
- Recording slot system: placeholder SVGs labelled with the exact target path,
  swapped at runtime when real media is dropped at `site/assets/media/*`.

[0.1.0]: https://github.com/MySetsuna/ridge/releases/tag/v0.1.0
