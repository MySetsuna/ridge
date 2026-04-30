# Changelog

All notable changes to **Ridge** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.1.0] — 2026-04-30

The first public release of Ridge.

### Added

- Recursive split panes — horizontal, vertical, nested without depth limit.
  Each pane is an independent terminal session with its own working directory
  and command history.
- Multi-workspace support. Each workspace keeps its own panes and processes
  alive when you switch away.
- Stable terminal experience across PowerShell, bash, zsh, and cmd. Unicode,
  clickable hyperlinks, scrollback that holds several megabytes of output.
- Embedded code editor as an alternative pane mode, sharing the same split
  layout as terminals.
- File explorer with create / rename / delete / drag-and-drop / keyboard
  navigation, plus "Reveal in file manager" via context menu.
- Cross-pane search panel — search and replace across every open workspace
  at once, with case / whole-word / regex toggles and glob filters.
- Git commit graph rendered directly from repository history, refreshing
  automatically when the working tree changes.
- Per-pane Git status badge showing branch, ahead / behind counts, and
  uncommitted change count, with an inline branch picker and "create branch"
  input.
- Source-control panel for staging, committing, and viewing diffs. Auto-detects
  git worktree links so the right HEAD is shown for each working tree.
- Claude Code agent collaboration — agents launched from a Ridge pane can
  list, name, create, and close panes, and read the working directory of any
  pane.
- Three built-in themes and a selectable editor font.
- Per-pane scrollback history viewer with search and "load older" paging.

### Improved

- Repository state refreshes from filesystem changes alone — no polling, no
  manual reload required.
- All confirm / input dialogs use Ridge's own window chrome, so prompts no
  longer interrupt the visual flow with native OS popups.
- File paths are normalised consistently across the app on Windows; the
  explorer no longer shows duplicate columns for the same directory.

### Known limitations

- Official installers for v0.1.0 are Windows-only. macOS and Linux users
  build from source.
- Agent collaboration is verified against Claude Code; other clients
  implementing the same multi-pane session protocol are not fully tested.
- Demo screenshots and recordings on the marketing site are still being
  captured; some are placeholders.

[0.1.0]: https://github.com/MySetsuna/ridge/releases/tag/v0.1.0
