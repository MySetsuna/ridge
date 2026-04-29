# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**Ridge** (codenamed Ridge) is a modern terminal emulator with split-pane functionality, embedded code editor, and Git visualization. It's built with Tauri v2 (Rust backend) + Svelte 5 (TypeScript frontend).

Key features:
- Terminal emulation via xterm.js with PTY support (portable-pty)
- Monaco Editor integration for code editing
- Recursive split-pane layout (horizontal/vertical)
- Git Graph visualization with Canvas rendering
- Multi-workspace support (independent terminal sessions)

## Commands

```bash
# Frontend development
pnpm dev        # Start SvelteKit dev server (port 1420)
pnpm build      # Build frontend for production
pnpm check      # Run SvelteKit sync + svelte-check

# Full Tauri application
pnpm tauri dev      # Run Tauri in development mode
pnpm tauri build    # Build production executable

# Rust-only
cargo check         # Verify Rust compilation
cargo fmt           # Format Rust code
cargo clippy        # Lint Rust code
```

## Architecture

### Frontend (src/)

- **Svelte 5 with runes** (`$state`, `$derived`, `$effect`)
- **Tailwind CSS v4** for styling
- **xterm.js** for terminal display
- **Monaco Editor** for code editing
- **svelte-splitpanes** for split layout

Key directories:
- `src/lib/components/` - UI components (SplitContainer, Pane, GitGraph)
- `src/lib/stores/` - Svelte stores for state management (`paneTree.ts`)

### Backend (src-tauri/)

- **Tauri v2** for native desktop integration
- **portable-pty** for PTY (pseudo-terminal) management
- **tokio** for async runtime
- **parking_lot::RwLock** for concurrent state access

Key modules:
- `src-tauri/src/lib.rs` - Tauri app setup, event bus, command registration
- `src-tauri/src/state.rs` - AppState with workspaces, terminals, scrollback
- `src-tauri/src/engine/pane_tree.rs` - Recursive split tree management
- `src-tauri/src/engine/pty.rs` - PTY handle management
- `src-tauri/src/commands/` - Tauri IPC commands (terminal, pane, git, workspace)

### Data Models

```typescript
// Frontend PaneNode (recursive tree)
type PaneNode =
  | { type: 'leaf'; id: string }
  | { type: 'split'; id: string; direction: 'horizontal' | 'vertical'; children: PaneNode[]; ratios: number[] }
```

```rust
// Backend PaneMode
enum PaneMode {
    Terminal,
    Editor { file_path: Option<PathBuf>, language: String },
}
```

### Communication

Frontend ↔ Backend via Tauri IPC:
- `invoke()` - Request/response commands
- `listen()` - Event subscriptions (e.g., `pty-output-{workspace_id}-{pane_id}`)

## Important Notes

- Workspaces are independent - each has its own PTY processes and pane ID namespace
- The `teammate` module provides an HTTP server for Claude Code integration
- The `tmux` binary (built from `src-tauri/src/bin/tmux.rs`) is a shim that allows using Ridge as a tmux replacement
- Frontend uses CSS custom properties (e.g., `var(--rg-bg)`, `var(--rg-fg)`) for theming
- The app runs in SPA mode with adapter-static fallback to index.html

## CWD path normalization (Windows)

All cwd strings stored in `paneCwdStore` must use **forward slashes** (`C:/code/ridge`, not `C:\code\ridge`). This ensures `syncWithPaneCwds` treats the same physical directory as one key regardless of which shell reported it.

- **Backend** (`pty.rs`): `normalize_cwd_str(&cwd.to_string_lossy())` is applied in all three `PaneCwdChanged` emit paths (main read loop, EOF flush, create_pane). `process.rs` has its own equivalent `normalize_cwd()`.
- **Frontend** (`paneTree.ts`): `normalizeCwd(s)` (`s.replace(/\\/g, '/')`) is called in `setPaneCwd` and `extractCwdsFromLayout`. All cwd writes go through one of these two entry points.
- **Do not** pass raw `PathBuf::to_string_lossy()` from Rust to the frontend without normalizing — on Windows, backslash paths cause duplicate Explorer columns for the same directory.

## WindDialog API

`src/lib/components/WindDialog.svelte` exports module-level promise helpers. Mount exactly once in `+page.svelte`.

| Function | Returns | Use |
|--|--|--|
| `alertDialog(opts)` | `void` | Non-interactive message |
| `confirmDialog(opts)` | `boolean` | Yes / No |
| `choiceDialog(opts + secondaryLabel)` | `'primary' \| 'secondary' \| 'cancel'` | Three-button (e.g. "始终允许" / "仅本次" / "取消") |
| `promptDialog(opts)` | `string \| null` | Text input |

Never use `window.alert`, `window.confirm`, or `window.prompt` — they render with OS chrome that breaks visual coherence inside Tauri.

## Sidebar / Explorer conventions

These patterns are used across `Explorer.svelte`, `FileTree.svelte`, `SourceControl.svelte`. Follow them when touching the sidebar:

- **Scrolling** — Complex tree regions (Explorer, SCM changes, Git graph) use `overlayscrollbars` via the `use:overlayScroll` Svelte action (`src/lib/actions/overlayScroll.ts`). The bar floats as an overlay, does not reserve gutter, and styles come from the `rg-os-theme` tokens in `app.css`. Do NOT layer `overflow-y-auto` + `rg-scroll-overlay` on top.
- **Horizontal tab scrolling** — `WorkspaceTabs` and `FileEditor` tab bar use `preset: 'horizontal-tabs'`. overlayscrollbars wraps children in `.os-viewport > .os-content`; any flex layout on the HOST is irrelevant. The `overlayScroll` action calls `applyContentLayout()` after init to inject `display:flex; flex-direction:row; white-space:nowrap; min-width:max-content` directly on `.os-content`. If tabs stack vertically it means `applyContentLayout` didn't run — check that `OverlayScrollbars()` created `.os-content` before the querySelector runs.
- **File tree navigation** — Each file-tree row stamps `data-rg-tree-path` and `data-rg-tree-column`. `Explorer.svelte` handles ArrowUp / ArrowDown / Home / End at the root `<div role="tree">` via `flattenVisiblePaths(column)` (see `fileExplorer.ts`), then `focus()`es the button and `scrollIntoView`es. Per-node keys (Enter / Arrow Left/Right / F2 / Delete) live in `FileTree.svelte`'s button.
- **Inline rename / create** — `FileTree.svelte` uses a local `editing: 'rename' | 'create-file' | 'create-folder' | null` state machine instead of browser `prompt()`. The name span swaps to an `<input>` for rename; a transient input row appears at the top of the directory's children for create. Enter commits, Esc / Blur cancels, `pendingEditCommit` guards against double-submit.
- **Expanded state persistence** — `fileExplorer.ts` serialises `expandedPaths` + `selectedPath` per `${workspaceId}:${cwd}` key to `localStorage['ridge-explorer-column:*']`. Capped at 500 paths per column.
- **Branch picker (SCM) dismissal** — `SourceControl.svelte` marks the picker trigger and dropdown with `data-rg-branch-picker="<root>"`. Global `mousedown` (capture phase) closes when click lands outside; Escape closes via global `keydown`. Only one picker open at a time.
- **Context menu keyboard** — `ContextMenu.svelte` supports Up/Down/Home/End navigation, Enter to activate, Esc to close, Right to open submenu, Left to close submenu. Menu items are tagged with `data-rg-ctx-index` for focus routing.

## Filesystem commands (Explorer right-click + inline edits)

`src-tauri/src/commands/project.rs` exposes:

| Command | Purpose |
|--|--|
| `rename_path(from, to)` | Move/rename a file or directory; refuses if `to` exists |
| `delete_path(path)` | Recursive delete for directories; plain `remove_file` for files |
| `create_file(path)` | Create an empty file; creates parent dirs; refuses if exists |
| `create_directory(path)` | `std::fs::create_dir_all`; refuses if exists |
| `reveal_in_file_manager(path)` | Platform-specific: Windows `explorer /select,...`, macOS `open -R`, Linux `xdg-open <parent>` |
| `copy_path(from, to, overwrite?)` | File or recursive directory copy via `walkdir` |
| `move_path(from, to)` | `fs::rename` first, falls back to copy + delete across drives |

All registered in `src-tauri/src/lib.rs` `invoke_handler!`.

## Sidebar Search tab

Third icon on the left rail (Ctrl+Shift+F also opens it). `SearchSidebar.svelte` mirrors VS Code's Search view:

- Searches every distinct cwd in `paneCwdStore` **in parallel** via `Promise.allSettled` + `text_search`, dedupes results. Glob diagnostics (`text_search_diagnostics`) fire concurrently and surface the red-ring immediately via `.then()` — before the search loop completes.
- Optional replace row (chevron toggle); replaces are bucketed per root and pass through `replace_in_files`.
- Toggle pills: case-sensitive / whole-word / regex.
- Glob filters: `compileGlobList` translates `**` / `*` / `?` to regex; `applyGlobFilters` runs on the JS side after results return.
- Auto-search debounce 400ms; Enter triggers immediately and cancels the pending timer.
- Each result row carries `r.line` / `r.column` and opens the file via `fileEditorStore.openFile(path, { line, column })`. The store stashes a one-shot `pendingReveal`; `FileEditor.svelte` consumes it after model swap.

## Pane title bar (`SplitContainer.svelte`)

Each leaf pane's header renders these affordances right-aligned (in order):

1. **`<PaneGitPill paneId>`** — branch + diff summary; click opens an inline picker that lazy-loads `git_list_branches`. "+ 创建新分支…" is an inline `<input>` (Enter submit, Esc cancel) — no `prompt()`. Ctrl-click pill jumps to the SCM sidebar tab.
2. **`Bot` button** — opens `ClaudeAgentLauncher` modal for the pane; Shift/Alt-click skips the prompt and launches bare `claude`.
3. **`History` button** — opens `ScrollbackHistoryModal` (read-only browser of pane scrollback bytes that may have scrolled past xterm's own buffer).
4. **`×` close pane**.

Per-pane git status lives in `paneGitStatus.ts`: a debounced (250ms) per-cwd resolver that parallels `find_git_repo_root` + `get_scm_status` + `git_diff_summary`, cached by repoRoot to coalesce panes that share a repo. Round-trip after staging/commit goes through `invalidatePaneGitStatusForRepo(repoRoot)`.

## Terminal scrollback (block-paged)

See `docs/TERMINAL_SCROLLBACK.md` for the full design. In-tree:

- Backend `state::PaneScrollback`: `VecDeque<Arc<Vec<u8>>>` blocks of `SCROLLBACK_BLOCK_SIZE = 64 KiB`, capped at `SCROLLBACK_MAX_BYTES = 4 MiB`. Each block carries a starting `seq` (monotonic byte counter) so callers page deterministically.
- Frontend commands: `get_pane_scrollback_tail(pane_id, max_bytes)` for newest bytes; `get_pane_scrollback_before(pane_id, before_seq, max_bytes)` for "load older". Legacy `get_pane_scrollback` is a deprecated shim — keep it working until phase-3 wraps.
- `Pane.svelte` mount-time replays the latest 256 KiB tail before live streaming kicks in.
- `ScrollbackHistoryModal.svelte` is the user-visible viewer: 256 KiB initial pull, "加载更早" pages 128 KiB at a time, ANSI is stripped for clean copy/search, in-modal search bar with n/N navigation and case-sensitive toggle.

## Plugin sidebar API

`$lib/stores/sidebarPlugins.ts` exposes `registerSidebarPlugin({ id, title, scope, component, order })`. Three scopes:

| Scope | Mount point | Props |
|--|--|--|
| `global` | Sidebar footer (always visible) | none |
| `workspace` | Beneath each workspace header in Explorer | `workspaceId` |
| `pane` | Bottom of each cwd column in Explorer | `workspaceId`, `paneId`, `cwd` |

`SidebarPluginRegion.svelte` walks the registry and mounts matching plugins. Built-in plugins live under `src/lib/plugins/` and are registered exactly once from `src/lib/plugins/index.ts` (a side-effect import in `+page.svelte`). **Don't auto-register inside a `.svelte` module script importing itself** — that breaks Vite's module graph.

## Modal z-index registry

To avoid stacking conflicts, modals follow this fixed table:

| Layer | z-index | Notes |
|--|--|--|
| `.rg-popup` dropdown menus | 9990 | `PaneGitPill`, `PaneRepoSwitcher`, recent workspaces; `position:fixed` with JS coords |
| `RidgeDialog` (alert/confirm/prompt) | 9998 | `position:fixed inset-0` |
| `ClaudeAgentLauncher` | 9997 | `position:fixed inset-0` |
| `ScrollbackHistoryModal` | 9996 | `position:fixed inset-0` |
| `ContextMenu` | 9999 | `position:fixed`, viewport-aware coords |
| `WindToast` | 10000 | Always above all modals |

All modals/overlays are mounted **outside** the root layout `<div>` in `+page.svelte` (as siblings), so `position:fixed` is always viewport-relative with no parent stacking-context interference.

When adding a new modal, claim a free slot and document it here.

## SCM git watcher

`src-tauri/src/commands/watch.rs` `GitWatcher` watches `.git/` for changes and emits `scm-repo-changed` (payload: repo root string) so `SourceControl.svelte` can auto-refresh without polling.

- Normal repos: watches `<root>/.git/` recursively.
- **Linked worktrees**: `.git` is a file (`gitdir: <real-git-dir>`). The watcher resolves the real git dir by parsing this file and watches that directory instead. Both normal repos and worktrees emit the same `scm-repo-changed` event.
- 500 ms debounce per repo root (client-side 250 ms additional debounce in `SourceControl.svelte` to coalesce rapid HEAD/index/refs writes on commit).
- `start_watching_repos` Tauri command — called from `SourceControl.svelte` after `discoverRepos`. Idempotent; unwatches stale roots automatically.

**SCM refresh policy** (round 64): `SourceControl.svelte` refreshes via exactly two active paths: (a) `paneCwdStore` change → 280 ms debounced `discoverRepos`, and (b) `scm-repo-changed` watcher event → per-repo debounced `refreshStatus` + `loadGraph`. Mount-time discover runs only when the cache is stale (>30 s). There is NO periodic timer and NO workspace-switch forced refresh — avoid re-adding those to prevent excessive IPC churn.

## Cargo zero-warning gate

`cargo build --lib` emits **0 warnings** as of round 19. Future-use APIs and legacy compatibility stubs are tagged with `#[allow(dead_code)]` and a one-line comment explaining why; do not remove the attribute without verifying nothing depends on the symbol externally. CI can safely run `cargo clippy -- -D warnings`.

## Next-loop planning

`docs/NEXT_LOOP_PLAN.md` is read/written by the `/loop` skill. Each iteration moves completed items to the history section and records the top candidates for the next pass. **Don't delete the history** — it's the audit trail for what's been tried.

## Claude Code Agent Teams (TmuxBackend)

Claude Code’s **TmuxBackend** shells out to `tmux` (the Ridge shim binary, built as `tmux`/`tmux.exe`) and expects **tmux-like** output, e.g. default `list-panes` lines (`0: [colsxrows] %0 (active)`) and `display-message -p ‘#{…}’`.

**Build the shim:** `pnpm run build:teammate-shim` — outputs `dist/teammate-shim/tmux` (or `tmux.exe` on Windows).

**Environment (required for the shim):** Ridge injects into PTY shells:

- `RIDGE_TEAMMATE_URL`, `RIDGE_TEAMMATE_TOKEN` — the shim POSTs/GETs the local teammate HTTP API
- `TMUX`, `TMUX_PANE` — so Claude treats the session as multiplexer-backed

Run Claude Code **from a terminal pane inside Ridge** so the agent inherits these variables. If the shim exits with “missing RIDGE_TEAMMATE_URL/TOKEN”, the child process did not inherit Ridge’s PTY env.

**Config:** `teammateMode` for Agent Teams is often read from **`~/.claude.json`** (global), not only project `settings.json`—confirm the effective mode is `tmux` or `auto` where intended.

**Windows / PATH / sandbox:** If you see “Could not determine current tmux pane/window” or `tmux` not found: ensure `tmux` resolves to the Ridge shim (e.g. after `pnpm run build:teammate-shim`, put `dist/teammate-shim` on `PATH`). Some Claude Code builds resolve `tmux` without relying on your shell `PATH`; set an explicit tmux binary path in Claude settings if available. Avoid launching Claude from directories or sandboxes that block the resolved `tmux.exe` path (see anthropics/claude-code issues on Windows cwd vs WinGet paths).

**Git Bash / MSYS:** Quoting can mangle `#{window_panes}` before it reaches the shim—prefer **PowerShell** or **cmd** / Windows Terminal for Claude Code when using the tmux backend.

**`list-sessions`:** The shim prints one line like real tmux: session index `0:` (matching the middle segment of `TMUX=/ridge/teammate.sock,0,{pane}`), dimensions `[120x80]`, and `(attached)` so tools that parse current session state do not treat the session as detached.

**`kill-pane`:** The shim POSTs `{ pane_index }` to `POST /api/v1/kill-pane`. Ridge removes the pane from its layout, tears down the PTY, and emits `teammate-layout-changed`. `-a` (kill-all) is a no-op to preserve at least one pane.

**`rename-window`:** The shim POSTs `{ pane_index, name }` to `POST /api/v1/rename-pane`. Ridge writes the name to `teammate_pane_titles` and emits `teammate-layout-changed` so the pane header updates immediately. This lets Claude Code label its panes (e.g. `tmux rename-window -t 1 "backend"`).

**`display-message` template variables:** Supported static vars include `#{pane_id}`, `#{pane_index}`, `#{pane_width}`, `#{pane_height}`, `#{pane_tty}`, `#{pane_pid}`, `#{pane_current_command}`, `#{window_id}`, `#{window_index}`, `#{window_panes}`, `#{window_name}`, `#{session_id}`, `#{session_name}`, `#{client_width}`, `#{client_height}`. Dynamic vars `#{pane_current_path}` and `#{window_panes}` query `GET /api/v1/list-panes?json=1` (which now returns `cwd` per pane).

**`list-panes?json=1`:** Returns `{ active_index, pane_count, panes: [{ index, pane_id, uuid, title?, cwd? }] }`. The `cwd` field is populated from `Pane.cwd` (the last OSC-7-reported path, forward-slash normalised). Useful for getting the current working directory of each pane.

**Smoke checks:** With teammate running and env set, run [`scripts/teammate-tmux-smoke.ps1`](scripts/teammate-tmux-smoke.ps1) (Windows) or [`scripts/teammate-tmux-smoke.sh`](scripts/teammate-tmux-smoke.sh) (Unix).

**Agent subprocess env:** If the error happens only when spawning a teammate and `tmux-shim.log` shows no new lines, Claude Code may be resolving `tmux` or running pane detection **before** the shim runs, or the child process may not inherit `TMUX` / `TMUX_PANE`. That path is controlled by Claude Code; ensure teammates are started from a context that inherits the same environment as the leader (see upstream issues on Windows TTY / in-process mode).