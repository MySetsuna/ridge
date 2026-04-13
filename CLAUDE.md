# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**Wind** (codenamed WarpForge) is a modern terminal emulator with split-pane functionality, embedded code editor, and Git visualization. It's built with Tauri v2 (Rust backend) + Svelte 5 (TypeScript frontend).

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
- The `wind-tmux` binary is a shim that allows using Wind as a tmux replacement
- Frontend uses CSS custom properties (e.g., `var(--wf-bg)`, `var(--wf-fg)`) for theming
- The app runs in SPA mode with adapter-static fallback to index.html