# Iteration 67 Contract — Explorer continuity and context identity

- Date: 2026-07-29
- Status: approved / implementation
- Requirements:
  - `REQ-EXPLORER-FREE-RESIZE-01`
  - `REQ-EXPLORER-FILE-CONTINUITY-01`
  - `REQ-EXPLORER-CONTEXT-ACTIONS-01`

## Delivery boundary

1. Explorer body resize
   - Pointer may continue across lower panels through window-level listeners.
   - `pointerup` and `pointercancel` release capture, listeners, resize state, and
     dragging classes.
   - Height persists once at drag end; never per frame.

2. First file open
   - Existing tabs retain the current dirty/clean reload contract.
   - A first open refreshes the node parent, resolves the returned canonical
     entry, then opens it.
   - A missing or directory replacement is not opened as stale file content.

3. Context actions
   - FileTree actions use the path captured when the menu opens.
   - cwd and pane-header menus expose absolute path, workspace-relative path,
     and reveal.
   - Pane-header actions capture `{workspaceId,paneId,cwd}` at menu-open time;
     action execution never substitutes the then-active workspace.
   - Until the backend exposes a distinct workspace root, the first stable cwd
     in workspace column order is the root; the root itself renders as `.`.

## Non-goals

- No global pane-split rewrite.
- No automatic dirty-buffer overwrite.
- No local reveal for Remote/shared paths.
- No new runtime watcher, polling loop, or per-frame persistence.

## Deterministic gates

- `src/lib/stores/explorerLayout.test.ts`
- `src/lib/stores/fileExplorer.test.ts`
- `src/lib/stores/fileEditor.test.ts`
- Svelte diagnostics for touched components/routes
- Focused Rust filesystem tests only if Rust code changes

## User-track evidence

The following remain observational and do not block deterministic code delivery:

- 60 Hz pointer feel on the user's actual Explorer composition.
- Windows Explorer selection behavior with the user's shell configuration.
- Permission-denied and cross-volume behavior on the user's actual volumes.

