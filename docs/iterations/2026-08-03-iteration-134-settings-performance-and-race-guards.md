# Iteration 134 - settings performance and race guards

Date: 2026-08-03

## Root cause

Opening the desktop settings panel mounted an eager `detect_available_shells`
call. On Windows that path probes `PATH`, may synchronously enumerate WSL
distros, and may invoke `vswhere`; it was unrelated to the default Appearance
tab. The overlay also applied a full-window `backdrop-filter`, forcing the live
terminal surface into an expensive blur/compositing pass.

Settings writes amplified the problem. Every `settingsStore` update reached the
default-CWD subscription, so theme, scrollback, font, and IME changes each
scheduled `set_user_default_cwd`. The terminal host settings port likewise
notified the theme bridge for fields that had not changed.

## Changes

- `SettingsPanel.svelte` schedules shell discovery only for the Terminal tab,
  after an idle/paint boundary; Agent profile preview and wallpaper URL work
  follow the same boundary. Generation checks reject results after tab/panel
  changes. In-flight shell work is not respawned and its loading flag always
  settles.
- Full-window settings blur was removed; layout/paint containment limits the
  modal's damage area. Theme previews derive in one pass, batch URL resolution,
  and use `loading="lazy"` plus asynchronous image decoding.
- `+page.svelte` now mirrors only `defaultCwd`, sends the initial value once,
  coalesces rapid text input to the latest value, and clears timers when the
  page is destroyed. Failed sends do not repeat on unrelated setting changes.
- `hostPorts.ts` deduplicates the `(scrollback, font family, default shell)`
  tuple before notifying the terminal theme bridge; the active theme id remains
  in the snapshot so theme changes still propagate. The bridge coalesces rapid
  theme notifications into one animation-frame push.
- `SettingsPanel.test.ts` guards the lazy boundary, no-blur contract,
  generation checks, image loading policy, and default-CWD deduplication.

## Evidence

- `pnpm exec vitest run src/lib/components/SettingsPanel.test.ts --reporter=dot`:
  1 file / 4 passed.
- `pnpm exec vitest run --reporter=dot`: 147 files / 1515 passed / 1 skipped.
- `pnpm check`: 0 errors / 0 warnings.
- `git diff --check`: clean.
- Pushed commits: `a3d6e81`, `042e30b`, `599a1d1` to `origin/main`.

## Boundaries

This closes the code-level settings-page eager-work and duplicate-sync paths;
it does not claim a measured WebView2 long-run heap soak, physical GPU-adapter
trace, or public Remote/Cloud deployment. No version bump or release was made
because `v0.1.54` consumed today's publication allowance. A native WebView2
trace should verify first-contentful settings open and 60-second idle memory
before the next release window.
