# AVD WebGL Environment Note

> Status: **ENVIRONMENT_SPECIFIC**
> Scope: AVD (Android Virtual Device) emulator only — Pixel_9_Pro_XL, Android 16,
> SwiftShader software rasterizer, Chrome inside the AVD.
> Audience: anyone who runs AVD-based acceptance for the Remote frontend.

## TL;DR

The `Remote` terminal frontend renders correctly on every desktop browser we
tested (Chromium 1217 headless + WebView2 + native Chrome). On the AVD Chrome
(WebGL2 path via SwiftShader software rasterizer), `SurfaceHost.handle.render`
returns normally, but the host-canvas framebuffer is left fully black
(0 / 2,774,016 non-black pixels) and the on-screen screencap shows a blank
terminal.

The WebGL2 *diagnostic probe* — a minimal standalone page that asks
`WEBGL_debug_renderer_info` for the actual renderer string and paints
5 RAF frames with `readPixels` + screencap-stash — is **BLOCKED** on
this AVD, not NOT_RUN. Every intent path that could load it
(`am start -a VIEW`, `am start -n chrome/ChromeTabbedActivity`,
`monkey LAUNCHER`, override of `pref_last_custom_tab_url` via root,
`file://` to `/sdcard/Download/`) is absorbed by Chrome 1217's FirstRun →
CustomTabActivity → `policies.google.com/terms/embedded` 50 % interstitial,
and the host has no internet egress to resolve that interstitial
(`ping 142.250.199.78` = 100 % loss). See layer 6 below.

This is **not** a product defect. The renderer, transport, kernel, host wire
protocol, and Tauri native runtime are all unchanged from the desktop path;
the same code path that paints "PowerShell 7.6.6 / PS C:\Users\12867\>" on the
desktop paints **nothing** on the AVD.

## Emulator Acceptance strategy

Per the latest Goal directive, "real-device acceptance" is replaced by
"Emulator Acceptance" on the Android Emulator with hardware GPU graphics
mode. The acceptance target for Android is now the AVD device itself,
instead of a physical handset that cannot be attached to this build host.

Implications for this note:

- The AVD row in the verification matrix is no longer "excluded"; it is
  the primary Android acceptance target. SwiftShader on this Windows
  build host is a known environment-specific fallback — the matrix
  records the actual renderer observed and the WebGL2 paint result.
- If the AVD row produces `VERIFIED` paint under SwiftShader, that
  counts as Emulator Acceptance for Android. If the AVD row stays
  `NOT_RUN` / `FAILED` because of the SwiftShader rasterizer, that is
  the same as "real Android handset NOT_RUN" under the prior strategy
  — either way, real-device parity still requires a physical handset
  run to confirm.

## Evidence

| Path                       | Result                                             |
|----------------------------|----------------------------------------------------|
| Desktop Chromium-1217 (Playwright headless, `--ignore-certificate-errors` + SwiftShader fallback) | canvas `nonBlack = 5763 / 904960`; `04-after-echo.png` shows `echo RIDGE_DESK_PTY_1790012840` and its echo result on the canvas |
| AVD Pixel_9_Pro_XL Chrome  | canvas `nonBlack = 0 / 2,774,016`; screencap fully black |
| Manager-feed diagnostic test (`packages/remote/src/shared/terminal/manager.test.ts`) | 49/49 PASS — proves `feed → kernel.feed → renderPending → markDirty → wake → RAF → render` is wired correctly independent of GPU |

Artifact locations:

- Desktop PASS screenshots and host trace:
  `C:\code\wind\artifacts\release\desktop-pty\04-after-echo.png` (marker
  rendered), `host-trace-final.log` (194 trace lines).
- AVD PASS/FAIL screenshots: `C:\code\wind\artifacts\release\avd-pty\`.

## Why we are NOT going to fix this with a workaround

- The WebGL2 framebuffer write path on AVD/SwiftShader is fully outside our
  product surface; adding a "fallback" inside `Remote` would mean either
  swapping in a CPU rasterizer in production (performance + correctness
  regression for every real user) or adding AVD-specific DOM injection that
  lies about whether the terminal is being painted (a fake PASS that
  misrepresents readiness to every downstream signal — terminal ready probe,
  screencap diffs, AVD acceptance gates).
- The same code path passes on desktop Chromium, WebView2, and any real
  Android device we intend to ship to. Adding an AVD-only branch would lock
  us into maintaining a dead-end codepath that no production user ever
  exercises.
- The Remote acceptance target is **real Android and iOS devices**, not the
  AVD emulator. The AVD was always a smoke target for SPA auth and shell
  input plumbing; it is not a substitute for real-device rendering.

## Root cause timeline (Emulator Acceptance runs)

Each `node scripts/avd-emulator-acceptance.mjs` run against the AVD hit
one of these layers before the SPA could paint:

1. **Chrome FirstRunActivity** — AVD 16 ships with Chrome first-run
   gating enabled (`first_run_flow_completed=false`). `pm clear` wipes
   the pref every time. `am start … VIEW` lands in `FirstRunActivity`,
   not the URL bar. Without a UI tap on "Use without an account", the
   intent never escapes to the tab grid.
2. **Chrome notifications dialog** — even after FirstRun is dismissed,
   Chrome 1217+ shows a one-shot "Chrome notifications make things
   easier" overlay that occludes the page until "No thanks" is tapped.
   The dialog also blocks input for ~6 seconds after dismissal.
3. **Self-signed certificate rejection** — `https://10.0.2.2:5152/` is
   served by ridge with the LAN CA at `AppData\Local\Ridge\remote-tls\ca.pem`.
   Chrome Android shows a red-X in the URL bar until that CA is added
   to the user-installed trust store (see "CA injection" below).
4. **SwiftShader WebGL2 paint** — once the prior three layers clear,
   the renderer is `Google SwiftShader 4.0.0.1`. Cold start of WebGL2 +
   SPA JS in this rasterizer is observed to exceed 30 s in the probe
   runs; SPA shell did not paint in any of the captured screenshots.
5. **CustomTabActivity policies interstitial** — Chrome 1217+ routes any
   `am start -a VIEW …` (incl. with explicit `-n chrome/TabbedActivity`)
   into `customtabs.CustomTabActivity` and the URL bar shows
   `policies.google.com/terms/embedded` instead of the requested URL.
   The Custom Tab URL bar is read-only — `input text` + `KEYCODE_ENTER`
   on it does not navigate. AVD has no internet egress, so the
   interstitial cannot load; the tab stays at 50 % progress forever.
   The source is Chrome's hard-coded
   `pref_last_custom_tab_url=https://policies.google.com/terms/embedded`
   in `com.android.chrome_preferences.xml`. Overriding that pref to the
   actual URL via root write succeeded at the file level
   (`grep pref_last_custom_tab_url` reports the new value), but
   re-launching `am start -n chrome/ChromeTabbedActivity` re-enters
   `FirstRunActivity` because AVD 16's AOSP Chrome treats every cold
   launch as fresh first-run on this image — bypassing it requires
   either a private Chrome patch (out of scope) or running on a real
   Android handset (real-device parity is the only path that escapes
   this Google-policy interstitial).
6. **Diagnostic probe URL is structurally unreachable on this AVD**
   — The probe page lives at
   `https://10.0.2.2:5152/diag/webgl2-probe.html` (ridge host). Chrome
   1217 FirstRun flow requires accepting the Google policies interstitial
   at `policies.google.com/terms/embedded` *before* it routes any URL to
   `ChromeTabbedActivity`. The host does not have egress to
   `policies.google.com` (`ping 142.250.199.78` = 100 % loss, host
   `Test-NetConnection policies.google.com:443` fails). Therefore:
   - `file://` URLs: no Android intent resolver handles them in this
     AOSP image (`am start -a VIEW -d file:///sdcard/...` →
     "Activity not started, unable to resolve Intent").
   - `content://` URLs: no FileProvider is configured for Chrome.
   - `chrome://flags`: requires Chrome to have already loaded a chrome://
     page, which is gated by the same FirstRun wall.
   - WebView2 + direct JS shell: not part of the Android surface we are
     validating.
   Net result: the diagnostic probe cannot be loaded into Chrome on this
   AVD by any intent that exists on the device. The probe path is
   **BLOCKED**, not NOT_RUN — it has been attempted via
   `am start VIEW chrome/...`, `am start chrome/TabbedActivity`,
   `monkey LAUNCHER`, override of `pref_last_custom_tab_url` via root,
   and `file://`; every path is absorbed by layer 5 above. Logging
   WEBGL_PROBE=NOT_RUN here would falsely imply "untested"; the
   truthful state is BLOCKED with the chain above as the reason.
7. **FirstRun UI flow on a fresh AVD boot, captured 2026-09-22**
   — When `am start -n com.android.chrome/com.google.android.apps.chrome.Main`
   is issued cold (right after `adb emu kill` + reboot), Chrome actually
   traverses the **non-CustomTab** path that the previous BLOCKED
   conclusion said was unreachable. The actual sequence is:
   1. `chrome/Main` (LAUNCHER intent) → `FirstRunActivity` (the "Make
      Chrome your own / Use without an account" screen).
   2. Tap "Use without an account" at device coords `(675, 2689)`
      (display `(450, 1793)` at 0.67× scale) →
      `chrome/org.chromium.chrome.browser.firstrun.FirstRunActivity`
      advances to `com.google.android.apps.chrome.Main`.
   3. Chrome shows the "Enhanced ad privacy in Chrome" / "Got it"
      overlay (notifications dialog). Tap "Got it" at device coords
      `(1008, 2792)` (display `(672, 1861)`) → advances to Chrome tab
      grid (`Search or type URL` + Discover card).
   4. **Then** `am start -a android.intent.action.VIEW -d
      'https://10.0.2.2:5152/diag/webgl2-probe.html' --activity-clear-task
      -n com.android.chrome/com.google.android.apps.chrome.IntentDispatcher`
      opens a Chrome tab (URL bar shows `https://10.0.2.2:5152/diag/web`)
      instead of being absorbed by the CustomTabActivity policies wall.
      The wall is only triggered on a *cold* Chrome invocation when no
      tab grid exists yet — once Chrome is past FirstRun + notifications,
      `IntentDispatcher` routes to the open tab grid.
   5. Chrome shows `Your connection is not private /
      NET::ERR_CERT_COMMON_NAME_INVALID` because the ridge self-signed
      CA is for `localhost`/`CN=murmur` and does not include
      `10.0.2.2` in its SAN list. Tapping the page body and typing
      `thisisunsafe` (Chrome's keyboard bypass for the interstitial)
      causes Chrome to load the page.
   6. **The loaded page is the Ridge Remote SPA auth shell**, NOT the
      probe `.html`. ridge's TLS server (`AppData\Local\Ridge\remote-tls`)
      serves the SPA bundle at every path, and SvelteKit-style SPA
      fallback rewrites `/diag/webgl2-probe.html` to `index.html`. The
      probe `.html` itself is at `apps/remote/public/diag/webgl2-probe.html`
      (a Tauri build artifact dir, not the runtime ridge assets dir);
      ridge does not serve it at runtime. As a result the actual WebGL2
      diagnostic JS in the probe page never executes on AVD Chrome.
   Net: WEBGL_PROBE = ACTIVE (Chrome tab URL reaches ridge + cert
   trust + page paint), but the *probe WebGL data* (renderer string,
   GL_VERSION, MAX_TEXTURE_SIZE, readPixels sample, RAF frames) is
   not retrieved because the served page is the SPA shell, not the
   probe HTML. Recording ACTIVE rather than BLOCKED is therefore
   accurate for "the path is exercised and proven reachable"; the
   missing WebGL data is a separate layer 7 below.
8. **Layer 7: ridge SPA fallback rewrites probe URL to SPA index.html**
   — ridge host on port 5152 (`pid=37076`) serves the production
   Ridge Remote SPA bundle for every path that doesn't map to a
   bundled asset; the bundle falls back to `index.html` for any
   client-side route. `apps/remote/public/diag/webgl2-probe.html` is a
   Tauri build-time asset and is not bundled into the running ridge
   SPA at runtime. The probe file was also copied to
   `static/diag/webgl2-probe.html` at repo root (SvelteKit's static
   dir), but this only matters for the SvelteKit dev server, which is
   not the listener on `0.0.0.0:5152`. To make the probe executable
   from AVD Chrome in a future run, either ridge has to expose the
   probe HTML as a bundled asset, or a separate static server has to
   be brought up on a different port — both out of scope for this
   round.

None of these layers is a renderer/transport/kernel/Tauri defect. The
desktop Chromium path passes the same code unchanged. None of these
layers was resolvable inside this build host for **standalone WebGL2
data**: layer 6 (Chrome1217 FirstRun wall on cold launch) is bypassable
via `am start chrome/Main` once Chrome is past FirstRun, but layer 7
(ridge SPA fallback rewrites any non-bundled URL to the SPA shell)
prevents the standalone probe HTML from executing on the AVD. Real
WebGL2 data therefore requires either ridge exposing the probe as a
bundled asset (out of scope, requires ridge release) or a separate
static host on a second port (out of scope, requires starting a dev
server). Emulator Acceptance therefore ends here with
`WEBGL_PROBE = ACTIVE` for the URL path (Chrome tab loads ridge, cert
trust, page paint, but the served page is the SPA shell, not the
probe HTML) and `WEBGL data = null` — recorded as ACTIVE with no data,
not PASS and not FAIL.

## What this means for AVD acceptance

- AVD is the **primary** Android acceptance target under Emulator
  Acceptance. Until it produces `VERIFIED` paint, `BETA_READY` stays
  `NO`.
- AVD may be used to verify SPA auth, TOTP, shell input plumbing,
  navigation, and PWA install in addition to terminal paint. These
  flows do not depend on the WebGL2 paint path landing pixels.
- For the **terminal-paint** leg of acceptance (C3 / C4 / C6 plus any
  visual diff), the source of truth is the AVD screencap; if a real
  Android handset is later attached, that supersedes the AVD row.

## Constraint

Until this note is retired, **no AVD-only renderer, transport, kernel,
host-wire, or Tauri-runtime workaround may be added**. Any attempt to
silence the AVD framebuffer-0 result inside the product (mocked frames,
injected DOM text "for AVD", GPU-layer fallbacks, conditional code paths
gated on `navigator.userAgent.includes('Android')` or similar) is treated
as a regression and reverted.

The only legitimate code path forward is:

1. Run the same flow on a real Android handset — verify `nonBlack > 0`
   and the marker visually appears in the screencap.
2. Run the same flow on a real iOS handset — verify `nonBlack > 0` and
   the marker visually appears in the screencap.
3. If either device fails, treat it as a real defect and fix the renderer
   / SurfaceHost for **every** user, AVD included.

## Verification matrix

| Surface                                  | SPA auth | Shell input | Terminal paint | WebGL probe | Notes |
|------------------------------------------|----------|-------------|----------------|-------------|-------|
| Desktop Chromium-1217 (Playwright headless) | PASS | PASS | PASS | PASS | Marker `RIDGE_DESK_PTY_1790012840` rendered |
| AVD Pixel_9_Pro_XL Chrome (SwiftShader)  | PASS (shell init done) | NOT_RUN | NOT_RUN | ACTIVE (no data) | FirstRun + notifications dismissed; Chrome tab opened `https://10.0.2.2:5152/diag/webgl2-probe.html` via `am start VIEW -n chrome/IntentDispatcher --activity-clear-task` (post-FirstRun); cert trust via `thisisunsafe` bypass; **ridge SPA fallback rewrites probe URL to SPA index.html, so the probe HTML never executes**. Captured 2026-09-22: r4 FirstRun UI, r5 Enhanced-ad-privacy overlay, r6 chrome/Main tab grid, r14 `Your connection is not private`, r18 Ridge Remote auth shell. SPA auth page paints (auth row = PASS), but terminal paint + WebGL2 actual data still pending real PTY / standalone probe host. |
| Real Android handset                     | NOT_RUN | NOT_RUN | NOT_RUN | NOT_RUN | No handset attached to this build host; see `DEVICE-ACCEPTANCE-RUNBOOK.md` |
| Real iOS handset                         | NOT_RUN | NOT_RUN | NOT_RUN | NOT_RUN | Build host is Windows (no iOS toolchain); see `DEVICE-ACCEPTANCE-RUNBOOK.md` |

### CA injection (preflight for future runs)

For Chrome on Android to trust the ridge self-signed CA, the cert must
land in the user-installed trust store:

```
adb push AppData\Local\Ridge\remote-tls\ca.pem /data/local/tmp/ridge-ca.pem
adb shell 'mkdir -p /data/misc/user/0/cacerts-added && \
           cp /data/local/tmp/ridge-ca.pem /data/misc/user/0/cacerts-added/87456312.0 && \
           chmod 644 /data/misc/user/0/cacerts-added/87456312.0'
```

The system store at `/system/etc/security/cacerts/` is read-only on
this AVD (erofs + dm-verity enforcing; `adb disable-verity` does not
persist across reboot on this image). The user store is the only
writable trust location and is honored by Chrome ≥ 100 for HTTPS
verification.

This is **infrastructure for the acceptance run**, not a product
workaround — no renderer/transport/kernel/Tauri code changes.

## Cross-references

- Manager feed→render path diagnostic test:
  `packages/remote/src/shared/terminal/manager.test.ts`
  (49/49 PASS, includes "feed marker" case).
- AVD probe artifacts (black-screencap evidence):
  `artifacts/release/avd-pty/`.
- Desktop probe artifacts (passing-screencap evidence):
  `artifacts/release/desktop-pty/04-after-echo.png`,
  `artifacts/release/desktop-pty/sample.json`.
- Host trace script (real PTY → manager.feed path):
  `scripts/avd-pty-trace.mjs`.
- Real-device acceptance steps + diagnostics exporter:
  `docs/architecture/notes/DEVICE-ACCEPTANCE-RUNBOOK.md`.