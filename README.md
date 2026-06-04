<p align="center">
  <img src="./static/ridge-mark.svg" width="64" height="64" alt="Ridge mark" />
</p>

# Ridge

<p align="center">
  <i>Like the ridges between rice paddies, split your work into plots — each field bearing its own harvest.</i>
</p>

<p align="center">「如田埂分畦，各耕其获」</p>

<p align="center">
  <a href="docs/README_CN.md">中文文档</a>
</p>

---

## Architecture

Ridge is a **native terminal workbench** built on Tauri v2 (Rust backend + Svelte 5 frontend). Every pane hosts an independent PTY session; the layout engine supports unlimited recursive horizontal/vertical splits. A WebWorker-hosted terminal renderer (Rust → WASM) drives the grid, with WebGPU as the primary backend and Canvas2D as the universal fallback.

```
┌─ Tauri v2 (Rust) ─────────────────────────────────────────┐
│  ┌─ commands/ ──┐  ┌─ engine/ ──────────┐  ┌─ remote/ ─┐ │
│  │ git · pane    │  │ pane_tree · pty    │  │ auth.rs   │ │
│  │ terminal      │  │ parser · cwd       │  │ mDNS      │ │
│  │ workspace     │  │ title · delta      │  │ WebSocket │ │
│  │ project       │  └────────────────────┘  └───────────┘ │
│  └───────────────┘                                         │
│  ┌─ teammate/ ──┐  ┌─ fs/ ──────┐  ┌─ db/ ──────────────┐ │
│  │ tmux shim    │  │ search     │  │ projects.db (SQLite)│ │
│  │ HTTP API     │  │ tree walk  │  └────────────────────┘ │
│  └──────────────┘  └────────────┘                         │
├───────────────────────────────────────────────────────────┤
│  ┌─ SvelteKit SPA (TypeScript) ───────────────────────┐   │
│  │  SplitContainer → @ridge/split (custom layout)     │   │
│  │  RidgePane (terminal shell + Monaco editor)         │   │
│  │  Sidebar: Explorer · Search · Source Control · Apps │   │
│  │  Multi-workspace with .ridge file persistence       │   │
│  │  TerminalManager → WebWorker → ridge-term (WASM)    │   │
│  └─────────────────────────────────────────────────────┘   │
└───────────────────────────────────────────────────────────┘
```

### Shared-core architecture (unified remote)

Pure domain logic is lifted out of the Tauri `src-tauri` crate into standalone,
**Tauri-free** workspace crates so the same code can run in any host — the
desktop app, the standalone `remote-server`, or the headless `ridge-cli`
(Linux/VPS). The desktop keeps its existing module paths via thin `pub use`
re-exports, so each relocation is a **zero-behavior-change** move:

- `ridge-core` — the command/`dispatch` + capability-policy core (already
  absorbed `fs/search` + `fs/tree`).
- `ridge-term` — the terminal kernel (VT parser + grid + render backends).
- `ridge-tmux` — the headless tmux session engine (see [Collaboration](#collaboration-agents--tmux)).

### Packages & crates (Cargo virtual workspace + pnpm monorepo)

| Crate / package | Tauri-free | Description |
|-----------------|:---:|-------------|
| `src-tauri` (`ridge`) | — | Desktop Tauri host: command handlers, engine, remote + teammate servers. Bins: `ridge` (app), `tmux` (shim), `remote-server`. |
| `packages/ridge-core` | ✓ | Runtime-agnostic command + workspace domain core: one `dispatch()` entry, one capability allow-list, shared by every host. |
| `packages/ridge-tmux` | ✓ | Headless, socket-namespaced **tmux session engine** (in-process PTY registry behind the `tmux` shim). Extracted from `teammate/native.rs`; its optional `http` feature exposes the shared `/api/v1/tmux/*` router mounted by **both** the desktop server and `ridge-cli tmux`. |
| `packages/ridge-term` | ✓ | Rust terminal kernel: VT parser, grid, scrollback, selection, search, Canvas2D/WebGPU backends. Compiled to WASM (lean on native via `default-features = false`). |
| `packages/ridge-cli` | ✓ | Headless remote host for Linux/VPS: device pairing + E2EE WebRTC PTY bridge (`remote`), and a headless tmux engine host (`tmux`). Reuses `ridge-core` + `ridge-tmux` (zero Tauri). |
| `packages/rg-split` | n/a | Pure-render split-pane layout component (frontend). Zero internal state; the store is the single source of truth. |

---

## Key Features

### Terminal
- Unlimited recursive **horizontal/vertical splits** with drag-to-dock
- Independent PTY per pane: each pane has its own shell process, CWD, and history
- **Delta-mode protocol** (P3): Rust-side parser emits postcard-compressed `GridDelta` frames via Tauri Channel — typical keyboard echo ~10 bytes on the wire vs ~3 KB JSON
- **Adaptive PTY output coalescing** with sub-1ms echo path: window scales from 0ms (<256 B) to 8ms (>=4 KB)
- Scrollback with search, "load older" paging, and user-scroll-lock detection
- Shell-integration prompt markers (`OSC 133;A` / `OSC 633;A`) for instant git-diff refresh
- Per-pane **shell switcher** (pwsh, cmd, bash, git-bash, WSL)
- Inline link resolution: clickable hyperlinks, file paths, git SHAs

### Workspaces
- Multiple **named workspaces** with independent pane trees, all processes kept alive across switches
- Single **global host canvas** (A.9): workspace switching is a pure DOM display toggle — no canvas reconfigure, no swap-chain clear, no black flash
- Save/restore via `.ridge` files in `~/ridge-workspaces/`
- Startup restore: re-open last session when launched from menu; override with CLI `ridge`

### Editor
- Monaco Editor as an **alternative pane mode** within the split layout
- Diff editor modal for inline file comparison
- Markdown preview with Mermaid diagram support

### Git (Source Control)
- Commit graph rendered from libgit2 history (not `git log` subprocess)
- Per-pane **Git status pill**: branch name, ahead/behind count, changed-file count
- **SCM sidebar**: stage/unstage/discard individual files, commit with message, branch switch, create tag, cherry-pick, revert
- Git operations: fetch, pull, push, sync against the SCM-selected repository
- Auto-refresh on filesystem change (notify crate) — no polling

### Search
- **Cross-workspace search** with regex, case-sensitive, whole-word, glob-filter toggles
- Replace-in-files across matches
- Text-search diagnostics sidebar

### Collaboration (agents & tmux)
- **Teammate server** (local HTTP API): external agents (Claude Code, etc.) can list/create/close panes, read pane CWD, and manage workspace layout
- **tmux shim**: a binary named `tmux` that translates standard tmux CLI into Ridge teammate HTTP calls — drop-in for Claude Code `teammateMode: tmux`. It routes through **two paths**:
  - **GUI-bridge** (default socket, pane/numeric targets): `split-window` and take-over map onto **visible** workspace split panes — what an agent team works in directly
  - **Native engine** (`-L`/`-S` custom socket, or named sessions): **headless**, socket-namespaced PTY sessions that run without occupying a pane, with faithful tmux `find-target` resolution, `capture-pane`, and `send-keys`
- **Native sessions** sidebar (desktop): lists the headless sessions and *summons* one into the current workspace as an adopted pane (shares the live PTY — no new shell)
- The native engine is the standalone, Tauri-free **`ridge-tmux`** crate. Its `/api/v1/tmux/*` router is shared *verbatim* by the desktop teammate server and by **`ridge-cli tmux`** (a headless-host subcommand) — so the same `tmux` shim drives headless sessions whether the host is the desktop app or a bare Linux/VPS box. Only GUI `summon` stays desktop-side
- Agent statistics dashboard in Settings panel

### Remote Control
Two browser surfaces, both served by the Rust backend:
- **Mobile console** (`src/remote`): a lightweight standalone PWA tuned for phones (virtual keyboard, canvas terminal), offline-cached via a service worker
- **Desktop-in-browser** (`RIDGE_WEB_REMOTE` build): the *full* desktop SPA built for a plain browser, with every `@tauri-apps/*` call redirected to WS-backed shims (`src/lib/transport/tauriShim/*`) so the desktop code runs untouched outside Tauri

Shared plumbing:
- **LAN-first**: TOTP authentication (RFC 6238, no external crate), mDNS discovery (`_ridge._tcp.local.` on port 5353), per-device blacklist, session management
- WebSocket binary terminal feed with per-client parser instances; the remote sidebar reuses the same shared components (`src/shared/sidebar/`) as the desktop app
- **Headless host** (`packages/ridge-cli`): a Linux/VPS binary with device pairing and an **E2EE WebRTC** PTY bridge (`ridge-cli remote`) — Ridge remote control with no desktop app running. `ridge-cli tmux` additionally hosts the headless tmux engine on the box, so an agent there can drive headless sessions through the `tmux` shim

### Theming
- Three built-in themes with CSS custom properties
- Theme system injected at splash-screen time (before SvelteKit hydration) so the first frame already matches the user's saved theme
- Monaco editor theme auto-synced from Ridge theme data

### Plugins
- Lightweight sidebar plugin system with global and workspace scopes
- Built-in plugins: **global status** panel; **Native sessions** panel (desktop-only — lists headless `ridge-tmux` sessions and summons them into the workspace; gated out of the web-remote build, where its host-only commands aren't available)

---

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Desktop framework | [Tauri v2](https://v2.tauri.app/) |
| Frontend | Svelte 5 + SvelteKit (SPA mode) + Tailwind CSS v4 |
| Terminal kernel | Custom Rust crate (`ridge-term`) → WASM via wasm-bindgen |
| GPU rendering | wgpu (WebGPU), Canvas2D fallback via OffscreenCanvas |
| Rendering host | Web Worker (`type: module`) for off-main-thread paint |
| Code editor | Monaco Editor 0.55 |
| Git | libgit2 (`git2` crate v0.19) |
| Database | SQLite via rusqlite (bundled) |
| Remote server | axum 0.7 (WebSocket + static file serving) |
| Auth | TOTP (SHA-256 HMAC, self-implemented RFC 6238) |
| IPC | Tauri `invoke()` for RPC, Tauri Channel for delta frames, Tauri events for PTY output |
| Serialization | postcard (binary) for grid deltas, serde_json for RPC |
| PTY | portable-pty 0.8 |
| Font icons | lucide-svelte |
| Diagrams | Mermaid 11 |
| Markdown | marked 15 |
| File watching | notify v6 + notify-debouncer-mini |
| Process discovery | sysinfo |
| Testing | Vitest (unit), Playwright (e2e), WebdriverIO (e2e-shell + perf) |

---

## Development

### Prerequisites
- **Rust** toolchain (latest stable)
- **Node.js** ≥ 18
- **pnpm** ≥ 8
- **Tauri v2** system dependencies ([setup guide](https://v2.tauri.app/start/prerequisites/))
- Windows: WebView2 runtime (bundled with Windows 10 21H2+)

### Commands

```bash
# Install dependencies
pnpm install

# Development (Vite HMR + Tauri hot-reload)
pnpm tauri dev

# Build release binary
pnpm tauri:build

# Run tests
pnpm test          # Unit tests (vitest)
pnpm e2e           # E2E tests (Playwright)
pnpm e2e:shell     # Shell-level integration tests (WebdriverIO)
pnpm e2e:perf      # Perf benchmarks (frame attribution, stress)

# Build WASM terminal kernel
node packages/ridge-term/build.mjs

# Build tmux compatibility shim
pnpm build:teammate-shim

# Build remote server (standalone binary)
pnpm build:remote-server

# Remote dev
pnpm dev:remote         # Run remote app dev server
pnpm build:remote       # Build remote app for production

# Type-check
pnpm check
```

### Project Structure

```
src/
├── routes/                     # SvelteKit pages (SPA entry)
│   ├── +page.svelte            # Main app shell (layout, sidebar, keyboard, theme, boot)
│   └── +layout.svelte          # Root layout (loader screen)
├── lib/
│   ├── components/             # Svelte UI components
│   │   ├── SplitContainer.svelte    # Recursive pane split layout
│   │   ├── RidgePane.svelte         # Terminal/editor pane with header
│   │   ├── FileTree.svelte          # File explorer tree
│   │   ├── Explorer.svelte          # Explorer panel with DnD
│   │   ├── SourceControl.svelte     # SCM panel (stage/commit/diff)
│   │   ├── GitGraph.svelte          # Commit graph visualization
│   │   ├── SearchSidebar.svelte     # Search & replace panel
│   │   ├── SettingsPanel.svelte     # Theme/font/shell settings
│   │   ├── WorkspaceTabs.svelte     # Workspace tab bar with drag reorder
│   │   ├── QuickOpen.svelte         # Ctrl+P file quick-open
│   │   ├── MarkdownPreview.svelte   # Markdown/Mermaid preview pane
│   │   ├── DiffEditorModal.svelte   # Side-by-side diff viewer
│   │   └── ...
│   ├── terminal/               # Terminal lifecycle & rendering
│   │   ├── manager.ts          # TerminalManager singleton (attach, render RAF loop)
│   │   ├── ptyBridge.ts        # PTY output → kernel.feed() bridge
│   │   ├── workerHostedRenderer.ts   # WebWorker proxy for off-main-thread render
│   │   ├── renderWorker.ts     # Worker-side render orchestration
│   │   ├── workerRendererBridge.ts   # Main ↔ worker protocol definitions
│   │   ├── linkSpans.ts        # Clickable-link detector
│   │   └── ...
│   ├── stores/                 # Svelte writable stores
│   │   ├── paneTree.ts         # Global pane tree state (split/close/dock/ratios)
│   │   ├── fileExplorer.ts     # File tree state
│   │   ├── scmCache.ts         # Git status cache
│   │   ├── paneGitStatus.ts    # Per-pane git pill data
│   │   ├── themes.ts           # Theme state & persistence
│   │   ├── settings.ts         # User settings
│   │   ├── searchState.ts      # Search panel state
│   │   └── ...
│   ├── transport/              # IPC abstraction
│   │   ├── tauri.ts            # Tauri invoke() transport
│   │   └── ws.ts               # WebSocket transport (remote)
│   ├── remote/                 # Remote-control UI
│   │   ├── RemotePanel.svelte  # QR code + session list
│   │   └── wsClient.ts         # Remote WS client
│   ├── plugins/                # Sidebar plugin system
│   └── utils/                  # Link resolver, markdown, ANSI, path utils
├── remote/                     # Standalone mobile remote app (Svelte, no SvelteKit)
│   ├── App.svelte
│   ├── MainApp.svelte
│   └── lib/
│       ├── terminalController.ts  # WebSocket-based terminal client
│       ├── VirtualKeyboard.svelte # Mobile on-screen keyboard
│       └── TerminalCanvas.svelte  # Canvas terminal for mobile
├── shared/sidebar/             # Transport-agnostic sidebar components
│   ├── SidebarFileTree.svelte
│   ├── SidebarGitPanel.svelte
│   ├── SidebarSearch.svelte
│   └── types.ts                # SidebarProvider interface
└── app.html                    # HTML shell (splash loader, boot globals)

src-tauri/
├── src/
│   ├── main.rs                 # Entry point → ridge_lib::run()
│   ├── lib.rs                  # App builder, event loop, command registration
│   ├── state.rs                # AppState (workspaces, terminals, PTY handles)
│   ├── types.rs                # GlobalEvent enum, PaneMode, PTY event types
│   ├── commands/               # Tauri IPC command handlers
│   │   ├── git.rs              # 25+ git operations (graph, diff, stage, commit, branches...)
│   │   ├── pane.rs             # Split, close, dock, toggle mode
│   │   ├── terminal.rs         # Create, activate, resize, write, scrollback, delta
│   │   ├── workspace.rs        # CRUD + save/restore workspace history
│   │   ├── project.rs          # File tree, search, replace, read/write, Claude/opencode history
│   │   ├── ridge_file.rs       # .ridge workspace file I/O + restore set
│   │   ├── settings.rs         # User default CWD
│   │   ├── theme.rs            # Theme data + splash init script builder
│   │   ├── watch.rs            # Git repo filesystem watcher
│   │   ├── fs_watch.rs         # General filesystem watcher
│   │   ├── remote.rs           # Remote control enable/disable, session/blacklist management
│   │   └── process.rs          # Foreground process / CWD detection
│   ├── engine/                 # Core engine
│   │   ├── pane_tree.rs        # Recursive split tree data structure + operations
│   │   ├── pty.rs              # PTY spawn with 2-stage activation
│   │   ├── parser.rs           # Native PaneParser (ridge-term on desktop)
│   │   ├── cwd.rs              # OSC 7 CWD tracking
│   │   └── title.rs            # OSC 0/1/2 title tracking
│   ├── fs/                     # Filesystem operations
│   │   ├── tree.rs             # Directory tree builder
│   │   └── search.rs           # ripgrep-style text search + filename search
│   ├── remote/                 # Remote control server
│   │   ├── server.rs           # Axum HTTP + WebSocket server, PTY fan-out
│   │   ├── auth.rs             # TOTP (RFC 6238) implementation
│   │   └── mdns.rs             # mDNS broadcaster (_ridge._tcp.local.)
│   ├── teammate/               # Agent collaboration (tmux shim backend)
│   │   ├── server.rs           # Local HTTP API: GUI-bridge routes + native-engine routes
│   │   ├── native.rs           # Thin re-export of the ridge-tmux headless engine
│   │   └── layout_event.rs     # Teammate layout-change events
│   ├── db/
│   │   └── projects.rs         # SQLite project store
│   ├── utils/                  # Error types, logging, PTY log, pane_id helpers
│   └── bin/
│       ├── tmux.rs             # Tmux shim: translates tmux CLI → Ridge teammate HTTP
│       └── remote-server.rs    # Standalone remote server binary
├── Cargo.toml
└── tauri.conf.json
```

---

## Release & Distribution

- **Windows**: NSIS installer + MSI, both with `PATH` environment registration
- Binary includes embedded `tmux` shim for Claude Code agent integration
- Bundled static assets: `ridge.theme` file + remote web app for LAN mobile access
- CI: GitHub Actions deploys the marketing site (`site/`) to GitHub Pages on push to `main`

---

## License

MIT License. Copyright (c) 2026 Jack Jiang and Ridge contributors.

---

<details>
<summary><b>☕ Buy Me a Coffee</b></summary>

<br>

<p align="center">
  <i>If Ridge has made your workflow smoother, consider buying me a coffee.</i>
</p>

<div align="center">

| WeChat 赞赏 | PayPal |
|:---:|:---:|
| ![WeChat Reward](./static/1.jpg) | ![PayPal Donate](./static/2.jpg) |
| 微信扫码赞赏 | PayPal Donate |

</div>

<br>

</details>
