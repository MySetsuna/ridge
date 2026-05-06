# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**Ridge** (codenamed Ridge) is a modern terminal emulator with split-pane functionality, embedded code editor, and Git visualization. It's built with Tauri v2 (Rust backend) + Svelte 5 (TypeScript frontend).

Key features:
- Terminal emulation via in-house **ridge-term** wasm kernel (VT/ANSI parser
  in Rust → Canvas2D renderer by default; opt-in WebGPU renderer behind
  the `webgpu` cargo feature, see "Render backends" below) with PTY support
  (portable-pty). xterm.js was retired in round 7; see `docs/term-rebuild/`
  for design history.
- Monaco Editor integration for code editing
- Recursive split-pane layout (horizontal/vertical)
- Git Graph visualization with Canvas rendering
- Multi-workspace support (independent terminal sessions)

## Render backends (ridge-term)

Two `RenderBackend` impls live in `packages/ridge-term/src/render/`:

- **`Canvas2dBackend`** — uses the browser's 2D canvas API. Always
  available; serves as the runtime fallback when WebGPU adapter
  acquisition fails.
- **`WebGpuBackend`** — uses wgpu 23 + the browser's WebGPU API. Single
  shader pipeline (`shaders/cell.wgsl`) + 256-layer texture-array glyph
  atlas + OffscreenCanvas-based rasterizer. **Ships in cargo's default
  feature set since 2026-05-05** (was opt-in `--features webgpu`).
  JS constructs via `await RenderHandle.newWithWebgpuFirst(canvas)`,
  which tries WebGPU and falls back to Canvas2D on adapter miss in Rust.
  JS additionally guards with `typeof RenderHandle.newWithWebgpuFirst ===
  'function'` for forward-compat with possible Canvas2D-only builds
  (`node build.mjs --no-webgpu`).

`AnyBackend` (in `render/mod.rs`) is the enum-dispatch wrapper that
lets `RenderHandle` hold `Renderer<AnyBackend>` and switch backends at
construction. `WebGpuBackend` covers every visual primitive
Canvas2dBackend does (cell bg+glyph, cursor in 3 styles, selection
overlay, hyperlink underlines) all through one render pass per frame.
WebGPU sets `RenderBackend::requires_full_frame() == true` so the
renderer marks every visible row dirty per tick — `LoadOp::Clear` wipes
the swap-chain texture each frame, so dirty-row diffing would otherwise
lose non-touched rows (the "only the line you're typing on shows up"
regression).

Status: Round 3 §4.1 functionally complete (2026-05-04) + §4.5 a-e
WebGPU integration shipped (2026-05-04). 2026-05-05: WebGPU promoted
to default backend with runtime adapter detection + Canvas2D runtime
fallback (no compile-time gate, no localStorage opt-in). §7.2 browser
real-run regression for the WebGPU path is the next gate before §4.3
(shared surface across panes) / §4.4 (perf benchmark).

§1.24 (2026-05-06): the resize path now propagates the kernel's alt-
screen state through to `resize_pane`; the backend skips its 250 ms
ConPTY resize-silence window for alt-screen panes so foreground TUIs
(claude / lazygit / Ink-based CLIs) don't have their SIGWINCH-driven
redraw bytes dropped. See `docs/term-rebuild/REPRO_alt_resize.md` for
the live repro recipe (`localStorage.RIDGE_DIAG='1'` exposes
`__RIDGE_KERNEL.lastResizeDiags()`; `localStorage.RIDGE_PTY_TRACE='1'`
dumps PTY-to-wasm chunks to the console).

§1.25 (2026-05-06): kernel-side reflow on resize is removed. The 200-
line `Grid::reflow_primary` rewrap algorithm (Round 3 §4.1 / TASKS §2.3
Phase 1) is gone; both primary and alt screens always go through naive
truncate/pad. Rationale: any application that cares about its layout
receives SIGWINCH from the PTY and emits its own redraw — shells
(PSReadLine / fish / zsh-zle) and TUIs (vim / less / claude code /
lazygit) all do this. A simultaneous kernel-side reflow races with that
redraw: cells the kernel has just relocated get overwritten by bytes
the app emitted under a different mental model of where they were,
producing visible "字符打架" (overdraw) and post-exit cursor drift.
Naive truncate/pad eliminates the race entirely. This matches xterm,
kitty, alacritty, iTerm2, and Windows Terminal — none of which reflow
on resize by default. Scrollback (paged-up history) shows historical
content at its original column width when wider than the new cols, the
same as before. `ResizeBranch` collapsed to a single `Naive` variant;
`is_alt` + `wipe_fired` carry the information the old triple encoded.

§1.25 also swaps `manager.ts::fitPane` ordering by alt state: on alt
screens, `kernel.resize` (which fires the §1.22 wipe) runs BEFORE
`resizeHandler` (which triggers PTY resize / SIGWINCH). This guarantees
the foreground TUI's redraw bytes always land on a freshly-cleared alt
buffer rather than racing the wipe and getting partially erased. On
primary the old PTY-first ordering is preserved for PSReadLine
absolute-cursor compatibility.

§1.26 (2026-05-07): primary-screen resize residue cleanup. Symptom:
after dragging a pane narrower on PowerShell + oh-my-posh, the
path-to-`>` gap collapses and ghost characters from the old prompt
linger past the new prompt's end. Root causes were threefold and the
fix has matching parts:
1. `grid.rs::resize_with_inline_tui` post-naive cleanup: when on
   primary AND dims changed, blank `cursor_row[cursor.col + 1 ..]` and
   every row strictly below the cursor (`cleared_below_cursor` in
   `ResizeDiag`). Cells AT cursor.col and to its left are preserved
   (raw shells without SIGWINCH-driven full redraws keep their
   in-progress text). Rows above the cursor are scrollback / prior
   command output and are never touched.
2. `engine/pty.rs::RESIZE_SILENCE_WINDOW_MS` 250 ms → 80 ms. Still
   suppresses ConPTY's viewport replay tail but lets PSReadLine's
   SIGWINCH redraw bytes (typical 10–50 ms post-SIGWINCH) actually
   reach the kernel.
3. `render/renderer.rs::tick` snapshot resize fires on *any* row-count
   change, not only growth. Required because Canvas2D's dirty-row
   diff would otherwise leave stale pixels on rows that vanished
   under a narrowing resize.

§A.3 alongside §1.26 adds the `resize_with_inline_tui(rows, cols,
inline_tui_active)` API. When the caller flags the pane as currently
hosting an inline TUI (Claude Code's Ink-based input box, etc.), the
*entire* visible primary region is wiped (mutually exclusive with
§1.26's partial cleanup). The detector lives in
`Grid::is_inline_tui_active_at(now_ms, cursor_visible)`: returns true
iff `!is_alt && !cursor_visible && now_ms - last_abs_csi_at_ms <
INLINE_TUI_DECAY_MS` (2 s decay). The parser records absolute-positioning
CSI dispatches via `Grid::note_absolute_positioning(now_ms)`. See
`docs/term-rebuild/REPRO_primary_resize.md` for the live repro recipe.

§1.27 (2026-05-07, diagnostic phase only): instrumentation for the
"莫名其妙置灰" (mysterious dim text) and "中文输入法预输入残留" (Chinese-
IME preedit residue) investigations. Two pieces shipped:
- `JsTerminal::cellsAt(row, col, len)` (`packages/ridge-term/src/lib.rs`)
  — returns per-cell `{ ch, codepoint, width, attrId, dim, bold,
  italic, underline, inverse, hidden, fg, bg }` for any range on the
  active screen. Lets devtools answer "is the grey I see backed by
  DIM-attributed cells, or is it stale rendering?"
- `RidgePane.svelte` `[ime] start/update/end` console logs gated on
  `localStorage.RIDGE_DIAG === '1'`. Captures composition lifecycle so
  textarea-overlay leaks can be told apart from grid-state writes.
The fix itself is deferred until live logs from these surfaces pin the
branch. See `docs/term-rebuild/REPRO_dim_residue.md` for the recipe.

§4.6 (2026-05-07, font-fallback only): `manager.ts:240`'s default
`fontFamily` already includes `"Segoe UI Emoji", "Apple Color Emoji",
"Noto Color Emoji"` after the monospace stack, so single-codepoint
emoji (🚀, ✅, 你好-style CJK) render in colour on both Canvas2D and
WebGPU (the WebGPU rasterizer's OffscreenCanvas honours the same font
stack). ZWJ composite emoji (👨‍👩‍👧, 🏳️‍🌈, 👨‍💻) still split into
multiple cells because `Cell.ch: char` only holds a single Unicode
scalar and the parser feeds the grid one codepoint at a time. Full
ZWJ support is queued as §4.7 and requires a cross-cutting refactor
(Cell glyph storage → grapheme cluster, parser → unicode-segmentation,
both backends → grapheme-keyed atlas).

### WebGPU is on by default — runtime detection

`pnpm tauri build` and `node build.mjs` ship the dual-backend wasm
bundle. At pane attach, `TerminalManager._makeHandle(canvas)`:

1. Checks `typeof RenderHandle.newWithWebgpuFirst === 'function'`
   (always true on default builds; false only on `--no-webgpu` bundles).
2. `await RenderHandle.newWithWebgpuFirst(canvas)` — Rust calls
   `instance.request_adapter(...)` + `adapter.request_device(...)`. If
   the browser doesn't expose `navigator.gpu`, no adapter responds, or
   device acquisition fails, the constructor returns `Err` and JS catches.
3. On the catch path (or step 1 false), JS falls back to
   `new RenderHandle(canvas)` — the synchronous Canvas2D constructor.

Both paths return a working `RenderHandle`. The user sees no failure;
the only difference is which backend `Renderer<AnyBackend>` holds.

To force Canvas2D for debugging:

```js
localStorage.RIDGE_WEBGPU = '0'; location.reload()
```

To rebuild a Canvas2D-only bundle (size-constrained builds):

```bash
cd packages/ridge-term && node build.mjs --no-webgpu
```

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
- **ridge-term wasm** (`@ridge/term-wasm`, source in `packages/ridge-term/`)
  for terminal display — `RidgePane.svelte` mounts a `<canvas>` and forwards
  PTY bytes through `TerminalManager` to the wasm kernel
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
3. **`History` button** — opens `ScrollbackHistoryModal` (read-only browser of pane scrollback bytes that may have scrolled past the wasm kernel's own buffer; backed by the backend's 4 MiB block store via `get_pane_scrollback_before`).
4. **`×` close pane**.

Per-pane git status lives in `paneGitStatus.ts`: a debounced (250ms) per-cwd resolver that parallels `find_git_repo_root` + `get_scm_status` + `git_diff_summary`, cached by repoRoot to coalesce panes that share a repo. Round-trip after staging/commit goes through `invalidatePaneGitStatusForRepo(repoRoot)`.

## Terminal scrollback (block-paged)

See `docs/TERMINAL_SCROLLBACK.md` for the full design. In-tree:

- Backend `state::PaneScrollback`: `VecDeque<Arc<Vec<u8>>>` blocks of `SCROLLBACK_BLOCK_SIZE = 64 KiB`, capped at `SCROLLBACK_MAX_BYTES = 4 MiB`. Each block carries a starting `seq` (monotonic byte counter) so callers page deterministically.
- Frontend commands: `get_pane_scrollback_tail(pane_id, max_bytes)` for newest bytes; `get_pane_scrollback_before(pane_id, before_seq, max_bytes)` for "load older". The legacy `get_pane_scrollback` (full-string shim) was removed post-round-7 once xterm retired and the wasm kernel adopted paged reads as the only path.
- `RidgePane.svelte` mount-time replays the latest 256 KiB tail before live streaming kicks in. It also seeds `oldestSeq` / `atOldest` from the chunk so `Shift+PageUp` past the wasm kernel buffer triggers `get_pane_scrollback_before` paging via `manager.prependScrollback` (TASKS §2.1).
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

`cargo build --lib --manifest-path src-tauri/Cargo.toml` emits **0 warnings** (last verified 2026-05-04). The wasm-side `cargo check --target wasm32-unknown-unknown --manifest-path packages/ridge-term/Cargo.toml --lib` also emits 0 errors / 0 warnings in both default and `--features webgpu` modes. Future-use APIs and legacy compatibility stubs are tagged with `#[allow(dead_code)]` and a one-line comment explaining why; do not remove the attribute without verifying nothing depends on the symbol externally. **However**, when the comment cites a now-shipped phase / round / mechanism (e.g. "used by phase-3 scroll-to-tail logic" after Phase 3 shipped via a different code path), the justification is dead — verify with grep, then delete the symbol. CI can safely run `cargo clippy -- -D warnings`.

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