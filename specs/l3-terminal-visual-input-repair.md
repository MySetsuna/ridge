---
id: L3-TERMINAL-VISUAL-INPUT-REPAIR
level: L3
title: Terminal visual coordinates, link opening, editor styles and input fidelity
status: LOCKED
lifecycle: ACTIVE
parent: L2-OBS-SRC-25a66342
code_targets:
  - packages/ridge-term/src/render/renderer.rs
  - packages/ridge-term/src/render/backend.rs
  - packages/ridge-term/src/render/surface_host.rs
  - packages/ridge-term/src/render/webgpu.rs
  - packages/ridge-term/src/term/grid.rs
  - packages/ridge-term/src/term/terminal.rs
  - packages/ridge-term/src/term/parser.rs
  - packages/ridge-term/src/lib.rs
  - packages/remote/src/shared/terminal/manager.ts
  - packages/remote/src/shared/terminal/linkSpans.ts
  - packages/remote/src/shared/terminal/tuiGate.ts
  - packages/remote/src/shared/terminal/linkOpenHost.ts
  - src/lib/components/RidgePane.svelte
  - src/lib/components/FileEditor.svelte
  - src/lib/components/DiffEditorModal.svelte
  - src/lib/terminal/ptyWriteQueue.ts
  - src/app.css
  - src-tauri/src/engine/parser.rs
  - src-tauri/src/engine/pty.rs
  - src/remote/lib/FileViewer.svelte
  - packages/ridge-term/src/term/delta.rs
  - packages/ridge-term/src/semantic.rs
  - packages/ridge-term/src/term/modes.rs
  - src/lib/monaco/ridgeTheme.ts
  - src/lib/monaco/trustedThemeStyles.ts
  - packages/ridge-term/src/input.rs
  - packages/ridge-term/src/term/cell.rs
  - packages/ridge-term/src/links.rs
  - packages/remote/src/shared/terminal/linkAffordance.ts
  - packages/remote/src/shared/terminal/renderTransaction.ts
test_targets:
  - packages/remote/src/shared/terminal/manager.test.ts
  - packages/remote/src/shared/terminal/manager.attach.test.ts
  - packages/remote/src/shared/terminal/linkSpans.test.ts
  - packages/remote/src/shared/terminal/tuiGate.test.ts
  - src/lib/terminal/ptyWriteQueue.test.ts
  - src/lib/terminal/rendererBackend.test.ts
  - tests/e2e/fileEditor.spec.ts
  - tests/e2e-shell/selection-tui-refresh.spec.ts
  - tests/e2e-shell/input-echo.spec.ts
  - scripts/terminal-five-fixes-e2e.mjs
  - packages/remote/src/shared/terminal/linkAffordance.test.ts
---

# Terminal visual coordinates and input fidelity

## Scope

Repair the five reported terminal/editor defects on the current branch. Preserve shell execution semantics, byte order, authentication, transport framing, persistence and unrelated work. Desktop, LAN and Cloud use the approved canonical terminal contract from ridge-signaling/src/terminal.rs. Incompatible peers fail explicitly; rendering consumes the bounded read-only viewport handoff without introducing another VT parser.

## Cursor

Anchor every painted cursor to its content row and cell, including across blink-off frames, viewport scrolling, output scrolling and partial scroll regions. During held-key navigation, every presentation frame reflects the current authoritative cursor cell immediately. Do not freeze the cursor until a guessed TUI quiet interval; only explicit VT synchronized-output transactions may defer presentation, with bounded recovery for an unclosed transaction. Preserve the last content coordinate rather than a stale viewport coordinate. Hide the cursor when its content leaves the viewport or is discarded. Structural reflow/screen changes invalidate obsolete anchors. Coalesced scrolls retain their actual movement count; the physical viewport transform still clamps to its height. Split native records at the existing u16 count limit instead of losing movement information.

## Links

ridge-term owns logical link detection, cell hit testing and link underline presentation for desktop and Remote; host adapters validate origin resources and execute opening. Scan complete logical lines before projecting targets to visual segments. Plain prose, MIME types and numeric ratios are not actionable file links. Preserve balanced URL/path punctuation. Only the validated target under the pointer receives link affordance on the existing modifier gesture; OSC8 metadata alone must not underline the whole terminal. Normal SGR text underline remains independent. Join only authoritative soft wraps, including bounded context beyond viewport edges (8192 cells per edge plus one boundary row). Keep hard breaks and real spaces separate. Wide-character wrap gaps carry an empty text override through the existing cell sidecar, contributing no logical character. Preserve grapheme text and map UTF-16 offsets directly to visible cells without a per-code-unit mapping allocation. Every visible segment opens the same complete validated target; preserve OSC8 precedence and existing URL/path validation.

## Editor styles

FileEditor and its integrated diff viewer keep readable syntax colors, chrome and cursor geometry during lazy mount and theme changes. Production CSP remains unchanged: when Monaco's internally generated head style.monaco-colors element is blocked, synchronize only that trusted theme through CSSOM constructable stylesheets. File contents and arbitrary style elements are never adopted. Remote FileViewer retains its external scoped styles, monospace read/edit surfaces, gutter and narrow viewport layout.

## TUI keyboard ownership

The native parser owns a durable TUI keyboard lease. Carry it through native delta and remote snapshots using the existing open-ended mode-state framing (internal code 65535). Mirror it before UI shortcut decisions. Cursor visibility or idle time does not release ownership; authoritative prompt/exit/reset signals do. ArrowUp/ArrowDown must never open shell history while the TUI owns input, including after idle and scrolling; restore shell history after a real return to shell.

## Multiline input

Unbracketed clipboard paste encodes each logical LF/CRLF/CR break as one CR Enter key, preserving source line order. Bracketed paste retains its body verbatim. Explicitly typed key sequences keep their existing encoding. Validate long multiline input against a real PowerShell 7.6.6 ConPTY, native parser, delta mirror and rendered order, using harmless numbered lines rather than replaying destructive user commands.

## Acceptance

Run npm test, npm run lint, npm run typecheck, terminal Rust tests, native parser regressions and the real PowerShell paste acceptance. Rebuild the consumed WASM/frontend assets. Run scripts/terminal-five-fixes-e2e.mjs against production output: verify actual Monaco colors under CSP, Remote FileViewer read/edit styles, GPU row order, and painted cursor/probe agreement through scrolling. Inspect final diff and deterministic stc verify/complete gates. Do not claim success from compilation, unit mocks or unchanged source alone.
