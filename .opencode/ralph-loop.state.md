---
active: true
iteration: 2
max_iterations: 30
completion_promise: "DONE"
started_at: "2026-05-29T00:00:00Z"
last_output: "Phase 0 complete: Renamed src/mobile→src/remote, static/mobile→static/remote, vite config, package.json, Rust server.rs/remote-server.rs/tauri.conf.json. Deleted +page.svelte, IdeScreen.svelte, TerminalScreen.svelte. Verified Rust compiles and Vite builds successfully."
---

Implement the Ridge Remote refactoring plan as discussed:

## Phase 0: Cleanup & Rename ✅ DONE

## Phase 1: Terminal Engine Optimization
- Create src/remote/lib/terminalController.ts with desktop-level optimizations
- Refactor TerminalCanvas.svelte to use TerminalController
- Full font stack, theme bridge, dirty-detection render loop, TUI feed coalescing

## Phase 2: Transport Abstraction
- Create src/lib/transport/types.ts, context.ts, tauri.ts, ws.ts

## Phase 3: Rust Backend
- Make git.rs *_sync() functions pub(crate)
- Add ~30 WS message handlers in server.rs

## Phase 4: Desktop Store/Component Refactoring
- Refactor fileExplorerStore, SourceControl, SearchSidebar, fsEvents

## Phase 5: Remote Reuse of Desktop Components
- Create RemoteSidebar.svelte, simplify MainApp.svelte, delete wsRemote.ts

## Phase 6: Code Splitting
- Configure vite.remote.config.js with manualChunks, lazy loading

Push to git when complete.