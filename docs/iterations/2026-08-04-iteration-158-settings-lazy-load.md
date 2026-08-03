# Iteration 158 — Desktop Settings cold-surface loading

## Scope

The desktop route imported `SettingsPanel` eagerly even while the panel was
closed. That pulled the settings/theme/icon/custom-theme graph into the first
route module and kept the panel's reactive lifecycle alive during startup.
This was a local, deterministic performance fix; it does not change settings
semantics or the WebGPU terminal path.

## Change

- Removed the static `SettingsPanel` import from `src/routes/+page.svelte`.
- Added a first-open dynamic import guarded by one shared promise.
- Kept the resolved component mounted after loading so later opens preserve
  section state and drafts without re-importing or rebuilding the panel.
- Added a source contract in `SettingsPanel.test.ts` that rejects the eager
  import and requires the lazy component path.

## Verification

- `pnpm exec vitest run src/lib/components/SettingsPanel.test.ts --reporter=dot`:
  5 passed.
- `pnpm check`: 0 errors, 0 warnings.
- `git diff --check`: passed (LF/CRLF conversion warnings are repository
  checkout normalization only).

## Remaining external gates

Physical desktop startup and settings/tab-switch frame traces, WebView2 heap
soak, public/physical Remote, dual-window workspace singleton, remote Host
latency, and complete kernel-domain authority remain required before any
release claim. No version bump or release was made.
