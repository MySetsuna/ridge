# Iteration 168 — Interaction parity and Agent communication registry

## Intake and research

- Notebook: `Ridge 项目现状、愿景与规划基线（2026-07-21）`
- Notebook ID: `66919cb9-1329-4ddf-955c-f426d15a9fe6`
- Recent conversation used: `a47d3199-c1f9-47f1-927c-ff2c4875b77d`
- NLM query was resource-limited; the transcript was read and treated as
  architectural guidance only. Local CodeGraph, source, and deterministic tests
  remain implementation authority.
- Approved requirements: `REQ-INTERACTION-PARITY-01` and
  `REQ-AGENT-COMMUNICATION-REGISTRY-01`. The pending intake records are marked
  approved; `docs/PENDING-REQUIREMENTS.md` is empty.

## Delivered slice

1. Access/share onboarding remains on the existing Kernel/Remote authority:
   Workspace tab/sidebar share actions, Hosts/Access panel, connect-progress
   banner, click attach, and drag-to-pane attach share one contract. The dialog
   closes before a slow connect and progress stays visible in the panel.
2. Remote mobile input no longer drops spaces or punctuation reported as
   `insertCompositionText`. Mouse-reporting TUIs receive touch press/drag/release
   and cancel cleanup; synthetic browser mouse duplicates are suppressed.
3. Desktop IME helper consumes the plain `input` fallback after composition and
   deduplicates only the matching composition commit, restoring CJK quote and
   punctuation input without duplicate PTY writes.
4. Explorer's vertical splitter can consume the complete free span, with the
   existing pointer-capture and listener cleanup retained.
5. Remote and desktop Agent history tabs are text/count-only (no history icon),
   Remote cards retain CWD and shared status/group/history projections, and the
   mobile control rail uses bounded, touch-sized icon/pending spacing.
6. Remote FileViewer wraps long file/diff lines and now renders image files via
   the authenticated `convertFileSrc` path with contained, safe-area-aware sizing.
   Desktop image overlay sizing also stays inside a narrow viewport.
7. Agent lifecycle communication now commits the registry only after confirmed
   pane/PTY activation, removes records on release/EOF/kill, rejects duplicate
   pane ownership, and performs a fresh online target preflight before writes.
   Failed registration rolls back the workspace map.

## Verification

- `pnpm check`: 0 errors, 0 warnings.
- Full Vitest: 149 files, 1560 passed, 1 skipped.
- Focused Vitest: 4 files, 33 tests passed; terminal input/mouse support slice
  adds a further 5 files/46 tests passed in the shared terminal helpers.
- `cargo check -p ridge --lib`: passed; existing dead-code/linker warnings only.
- `cargo test -p ridge --lib teammate::profiles --quiet`: 2 passed.
- `cargo test -p ridge-term input --quiet`: 36 passed.
- `git diff --check`: passed.
- Physical notch/PWA, real Grok/TUI behavior, public WebRTC, WebView2 heap soak,
  and multi-window/Host E2E remain runtime evidence gates; no local test is
  promoted to physical-device evidence.

## Closeout

The iteration note is archived here; no pending requirement remains. Release
is deliberately not created from this code-only slice: versioned release still
requires a clean, pushed worktree, aligned version sources, annotated tag, and
the complete desktop asset matrix. Remote/cloud publication remains a separate
release line.
