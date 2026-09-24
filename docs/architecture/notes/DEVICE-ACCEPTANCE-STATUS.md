# Device Acceptance Status (snapshot)

> Snapshot of the Remote frontend acceptance matrix as of this commit.
> This file is the single source of truth for "where do we stand on
> device acceptance?" — when a real device produces new evidence, the
> corresponding row gets updated here and in
> `AVD-WEBGL-ENVIRONMENT-NOTE.md`.
>
> Result vocabulary is exactly four values per the v0.1.89 device-acceptance
> hold: **VERIFIED**, **PARTIAL**, **FAILED**, **NOT_RUN**.
> No other value is allowed. Replacing any cell with `VERIFIED`
> requires an actual run on the corresponding device plus a
> `diagnostics.json` produced by `scripts/export-diagnostics.mjs`.

## Strategy

Per the latest Goal directive, "real-device acceptance" is replaced by
"Emulator Acceptance" on the Android Emulator with the hardware GPU
graphics mode (the host build still uses `hw.gpu.mode=host`, but
`Pixel_9_Pro_XL` falls back to SwiftShader on this Windows host; see
`AVD-WEBGL-ENVIRONMENT-NOTE.md`). iOS still has no toolchain on this
Windows host. Real Android / real iOS rows stay `NOT_RUN` until
hardware arrives.

## Status (Emulator Acceptance)

| Surface                                  | C3       | C4       | C5       | C6       | Long-history | Notes |
|------------------------------------------|----------|----------|----------|----------|--------------|-------|
| Desktop Chromium-1217 (Playwright headless) | NOT_RUN  | NOT_RUN  | NOT_RUN  | VERIFIED | NOT_RUN      | Software-keyboard text input + Enter → shell → marker rendered on canvas. TUI-mouse / scrollback gestures / pinch / orientation / IME composition / PWA / backgrounding NOT verified. `artifacts/release/desktop-regression/1790039218161/` |
| AVD Pixel_9_Pro_XL Chrome (SwiftShader)  | NOT_RUN  | NOT_RUN  | NOT_RUN  | NOT_RUN  | NOT_RUN      | Emulator GPU = SwiftShader 4.0.0.1 (host GPU acceleration did not engage). SPA shell did not finish loading within 30s; canvas stays splash. See `AVD-WEBGL-ENVIRONMENT-NOTE.md` and `artifacts/release/avd-emulator/1790042792162/`, `…/1790042923426/`, `…/1790043009945/`. |
| Real Android handset                     | NOT_RUN  | NOT_RUN  | NOT_RUN  | NOT_RUN  | NOT_RUN      | No handset attached to this build host |
| Real iOS handset                         | NOT_RUN  | NOT_RUN  | NOT_RUN  | NOT_RUN  | NOT_RUN      | Build host is Windows (no iOS toolchain) |

Long-history is its own sub-matrix (per v0.1.89 §4):

| Lines    | First-paint | A→B→A | Continuous-burst | Scrollback-up | Reconnect | Notes |
|----------|-------------|-------|------------------|---------------|-----------|-------|
| 100      | NOT_RUN     | NOT_RUN | NOT_RUN        | NOT_RUN      | NOT_RUN   | Real-device only |
| 500      | NOT_RUN     | NOT_RUN | NOT_RUN        | NOT_RUN      | NOT_RUN   | Real-device only |
| 1000     | NOT_RUN     | NOT_RUN | NOT_RUN        | NOT_RUN      | NOT_RUN   | Real-device only |
| 5000     | NOT_RUN     | NOT_RUN | NOT_RUN        | NOT_RUN      | NOT_RUN   | Real-device only |

iOS Safari / PWA sub-matrix:

| Dimension            | Safari | PWA standalone | Notes |
|----------------------|--------|----------------|-------|
| Login                | NOT_RUN | NOT_RUN       | Real-device only |
| Terminal attach      | NOT_RUN | NOT_RUN       | Real-device only |
| Background → resume  | NOT_RUN | NOT_RUN       | Real-device only |
| Lockscreen → resume  | NOT_RUN | NOT_RUN       | Real-device only |
| Chinese IME          | NOT_RUN | NOT_RUN       | Real-device only |
| viewport / safe-area | NOT_RUN | NOT_RUN       | Real-device only |
| Keyboard occlusion    | NOT_RUN | NOT_RUN       | Real-device only |

Cell legend:

- **VERIFIED** — concrete artifact on disk plus a sanitized
  `diagnostics.json` produced by `scripts/export-diagnostics.mjs`.
- **PARTIAL** — some sub-steps passed, others failed or untested.
- **FAILED** — concrete reproduction on the surface; reproduce recipe
  captured in a CHG candidate document.
- **NOT_RUN** — surface exists but no run has happened on this build
  host. The runbook section lists the exact commands that would flip
  the cell to `VERIFIED`.
- **NOT_APPLICABLE** — surface does not exercise the dimension (no
  current row uses this; reserved).

> Important: "Desktop Chromium VERIFIED" above counts only the
> software-keyboard → shell → canvas marker render path on a desktop
> headless browser. It does **not** count as a real Android or real
> iOS handset VERIFIED — those rows stay `NOT_RUN` until a physical
> device run lands here per the runbook.

## Gate

```
BETA_READY = NO
```

`BETA_READY` flips to `YES` only when **every** row that is not
`NOT_APPLICABLE` is `VERIFIED`. Under the Emulator Acceptance
strategy, that requires the AVD row to produce a `VERIFIED` outcome
for C3 / C4 / C5 / C6 / Long-history on the AVD surface — which is
blocked by the SwiftShader WebGL2 environment.

## While `BETA_READY = NO`

- Renderer / transport / kernel / host wire protocol / Tauri native
  runtime: no new feature work. Already-passing desktop paths may
  still receive defect fixes **only** when triggered by a real-device
  reproduction (v0.1.89 §7 narrow-CHG rule).
- Tests may grow; product code paths must not, except in a dedicated
  CHG.
- No push / tag / release / deploy of the Remote frontend to any
  channel that reaches users (per the standing constraints on this
  branch).

## OPEN_CHANGES (v0.1.89)

None — no real-device reproduction has been reported this session.
If a future run finds a defect, add a section here with the device,
environment, reproduce recipe, trace location, and minimal root-cause
hypothesis before any product code change lands.

## AVD environment evidence

Recorded on the host by `scripts/avd-emulator-acceptance.mjs`:

- emulator image: Pixel_9_Pro_XL (AVD)
- Android version: 16 (API 36)
- graphics mode: `hw.gpu.mode=host` (configured) → renderer = Google
  SwiftShader 4.0.0.1 (fallback)
- WebGL renderer: `Google (Google Inc.), Android Emulator OpenGL ES
  Translator (Google SwiftShader), OpenGL ES 3.0 (OpenGL ES 4.0.0.1)`
- screenshots: `artifacts/release/avd-emulator/1790042792162/01-loaded.png`
  through `…/07-final.png`, plus `…/1790042923426/`,
  `…/1790043009945/`. SPA shell did not render in any of these runs
  (Chrome stuck on splash under SwiftShader cold start).
- diagnostics: `artifacts/release/avd-emulator/<run>/diagnostics.json`

## To update this file

1. Run the device flow per `DEVICE-ACCEPTANCE-RUNBOOK.md`.
2. Run `node scripts/export-diagnostics.mjs --source <trace> --out
   <run-dir>/diagnostics.json`.
3. Capture the screencaps named per the runbook section (e.g.
   `c3-after-echo.png`, `c5-offline.png`).
4. Flip the corresponding cell to one of `VERIFIED` / `PARTIAL` /
   `FAILED` here **and** in `AVD-WEBGL-ENVIRONMENT-NOTE.md`. Both
   files must change in the same commit.
5. If the result is `FAILED` or `PARTIAL`, open a CHG candidate
   section under "OPEN_CHANGES" with the seven required fields from
   the v0.1.89 §7 rule.
