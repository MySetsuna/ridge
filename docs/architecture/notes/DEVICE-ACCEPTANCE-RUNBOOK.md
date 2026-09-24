# Device Acceptance Runbook (Android + iOS)

> Purpose: prove the Remote frontend renders and behaves correctly on
> **real devices**, not on the AVD emulator (which is excluded per
> `AVD-WEBGL-ENVIRONMENT-NOTE.md`).
>
> Status (as of this commit): **NOT_RUN** — no real Android handset was
> attached to the build host and the build host is Windows (no iOS
> toolchain). Runbook is ready; rows get filled in only after a real
> device is connected.
>
> Hold rule: while this runbook has rows with `NOT_RUN` or `PARTIAL`,
> `BETA_READY = NO` and no new feature work may open a new code path
> that has not already passed on the desktop browser.

## 0. Pre-flight (host machine + device)

1. `target\test-rdg\release\ridge.exe` is built (workspace keeps this
   current).
2. SPKI of the host TLS cert is at
   `artifacts\release\avd-acceptance\spki.txt`. The same key works for
   real-device Chrome — paste the SPKI into the device's
   `chrome://certificate-manager/?syncType=device&usesTo=Desktop&useFor=Web+requests`
   (Android) or install the CA profile via Settings → General → VPN &
   Device Management → Install Profile (iOS).
3. Two free ports: one for the host (default `5120`), one for the
   artifacts-side reverse tunnel if needed.
4. Device prep:
   - **Android**: USB debugging enabled; `adb devices` shows
     `<serial>    device` (not `unauthorized`).
   - **iOS**: device unlocked, "Trust this computer" accepted, Developer
     Mode enabled, Safari set as default for the test origin.

## 1. Android — C3 / C4 / C5 / C6

```
# 1a. confirm device
adb devices
# expect: <serial>    device

# 1b. forward the host port so the device can reach it
adb reverse tcp:5120 tcp:5120

# 1c. open Chrome on the device and navigate to the SPA
adb shell am start -a android.intent.action.VIEW \
  -n com.android.chrome/com.google.android.apps.chrome.Main \
  -d https://localhost:5120/
```

SPA auth: type the TOTP that `test-rdg` printed on stdout (the script
logs `TOTP: <6 digits>` on first start; refresh TOTP every 30 s as the
gate window expires).

### C3 — scrollback semantics on real GPU

**Step.** After the shell lands (`PS C:\...>` prompt visible and
rendered), drive a marker echo via the device's keyboard:

```
adb shell input text "echo%20RIDGE_AND_C3_<epoch>"
adb shell input keyevent 66      # Enter
```

Capture the screen after ~1.5 s:

```
adb shell screencap -p /sdcard/c3.png
adb pull /sdcard/c3.png artifacts/release/real-device/<run-id>/c3-after-echo.png
```

**Expected.**
- The terminal pane renders the typed command and the echoed marker on
  the device's screen (visual diff against the marker).
- A second `adb shell screencap` taken 5 s later still shows the marker
  on screen — the WebGL framebuffer was not blanked by a repaint cycle.
- `host-trace-final.log` contains `[ridge-trace]` lines for the pane
  frame delivery covering both `output_seq` increments.

**Evidence required.**
- `c3-after-echo.png` (≤ 2 MB).
- `host-trace-final.log` (full trace, ≤ 5 MB).
- A second screencap `c3-after-5s.png` taken 5 s after `Enter`.
- Diagnostics export (see §3) for the pane that received the marker.

### C4 — long input / multi-pane

**Step.** Type a multi-line command with at least three distinct output
lines:

```
adb shell input text "echo%20a%3B%20echo%20b%3B%20echo%20c"
adb shell input keyevent 66
```

Capture the screen and the host trace.

**Expected.**
- All three echo lines appear in order on the device's terminal grid
  (a, b, c — each on its own line, in order).
- The pane's `output_seq` in the diagnostics export increases by at
  least 3 (one per echo), with no drops.
- No scroll glitch: the terminal pane does not flash to blank between
  successive echoes.

**Evidence required.**
- `c4-multi-echo.png`.
- `host-trace-final.log` covering the multi-line input.
- Diagnostics export showing the `output_seq` deltas.

### C5 — connection-loss recovery

**Step.** With the SPA connected, kill the host process (or take the
host offline for ≥ 10 s) and bring it back:

```
# find the host PID
adb shell ss -tn 2>/dev/null | grep 5120
# on the host machine:
taskkill /F /PID <host-pid>
# wait ≥ 10 s, then restart the host
target\test-rdg\release\ridge.exe host --port 5120
```

Bring Chrome back to the foreground and observe the SPA.

**Expected.**
- The SPA shows the offline banner within 2 s of the host going away.
- When the host returns, the SPA reconnects automatically within 5 s;
  the diagnostics export shows `reconnect_state` transitioning
  `online → reconnecting → online`.
- The terminal pane's last visible frame remains on screen through the
  reconnect — no black flash.

**Evidence required.**
- `c5-offline.png` (SPA in offline state).
- `c5-online.png` (SPA after reconnect, shell prompt re-rendered).
- Diagnostics export with at least one full `online → reconnecting →
  online` transition and the matching timestamps.

### C6 — IME / virtual kbd

**Step.** On the device, tap the SPA kbd toggle in the toolbar. Then
type via the on-screen keyboard:

```
adb shell input text "echo%20RIDGE_AND_C6_<epoch>"
adb shell input keyevent 66
```

**Expected.**
- The shell input area updates as the IME emits composition events
  (visible in screencaps taken every 250 ms while typing).
- The host log shows the keystroke bytes reaching the PTY.
- The final echo on the device's screen matches the typed command.

**Evidence required.**
- `c6-ime-typed.png` (mid-typing).
- `c6-ime-final.png` (after Enter).
- Host log fragment showing the IME-emitted bytes.
- Diagnostics export for the pane during the IME session.

## 2. iOS — backgrounding / PWA / IME

Requires macOS build host with Xcode + libimobiledevice, plus an iOS
device with Safari.

```
# on macOS:
idevice_id -l
# confirm device
ios-deploy --detect           # alt
```

Serve the SPA over HTTPS with the same SPKI cert (the host already
listens on `:5120`; expose via `ngrok` or local network) **or** push the
PWA build to the device and open it from the home screen.

### 2.1 PWA install + launch

**Step.** In Safari, navigate to the SPA origin. Tap Share → Add to
Home Screen. Re-launch from the home-screen icon.

**Expected.**
- The app launches in standalone mode (no Safari chrome, no URL bar).
- The TOTP gate still works (refresh TOTP from the host's `TOTP: ...`
  line).
- After auth, the terminal canvas paints marker bytes (real Metal GPU;
  no SwiftShader).

**Evidence required.**
- `ios-pwa-launch.png` (standalone chrome absent).
- `ios-pwa-shell.png` (after TOTP, shell prompt visible).
- A marker echo screenshot `ios-pwa-marker.png` (after
  `echo RIDGE_IOS_PWA_<epoch>`).

### 2.2 Backgrounding

**Step.** Send the PWA to background (Home button / swipe up), wait
≥ 30 s, resume.

**Expected.**
- The WebSocket reconnects automatically within 5 s of resume.
- The diagnostics export shows a single `online → reconnecting →
  online` transition during the backgrounding window.
- Pane state survives: terminal grid content is intact after resume;
  no "stuck" blank canvas.

**Evidence required.**
- `ios-bg-resume.png` (after resume, shell prompt re-rendered).
- Diagnostics export with the backgrounding transition and timestamps.

### 2.3 IME

**Step.** Open an IME (e.g. Chinese keyboard — Pinyin), switch input
modes, type a marker:

```
# via macOS host + idevice tools:
idevicekeyboard input text "echo RIDGE_IOS_IME_<epoch>"
```

**Expected.**
- The shell input area updates as the IME composition events arrive.
- The host log shows the IME-emitted bytes reaching the PTY.
- The final echo matches the typed command.

**Evidence required.**
- `ios-ime-mid.png` (mid-typing).
- `ios-ime-final.png` (after Enter).
- Diagnostics export for the IME session.

## 3. Diagnostics export (sanitized)

Every C-step above requires a sanitized diagnostics export committed
alongside the run. The exporter must **only** include:

| Field           | Source                              | Notes                                  |
|-----------------|-------------------------------------|----------------------------------------|
| `session_id`    | localStorage `ridge-session-id`     | opaque per browser session             |
| `host_id`       | remote host identity handshake       | opaque (not user-bound)                |
| `runtime_epoch` | host / kernel epoch counter         | monotonic integer                      |
| `pane_id`       | pane key (`ws:ws<id>:pane<idx>`)     | opaque per session                     |
| `output_seq`    | per-pane sequence counter           | monotonic per pane                     |
| `reconnect_state` | `online` / `reconnecting` / `offline` | enum                                  |

**MUST NOT** include:
- TOTP value (the 6-digit code from `TOTP: ...` lines).
- SPA session token (the long-lived bearer cookie / localStorage value).
- Any user-typed content (shell commands, file paths, file contents).
- Host IP / hostname / MAC / BSSID.
- Device serial, advertising ID, IMEI.
- Cert private key, SPKI pin (the public SPKI is already in
  `spki.txt` and may be referenced by filename only).

The exporter must redact any string that matches these patterns in
free-form log lines before writing:

- `\b\d{6}\b` inside the `[ridge-trace] totp=...` context → drop.
- `Bearer [A-Za-z0-9._-]+` → drop.
- `token=[A-Za-z0-9._-]+` → drop.
- `file://[^\s]+` → drop.
- Any string that appears in the user's `~/.ssh/known_hosts` /
  `%USERPROFILE%\.ssh\known_hosts` → drop.

Run exporter script:

```
node scripts/export-diagnostics.mjs \
  --run artifacts/release/real-device/<run-id> \
  --source host-trace-final.log \
  --out   artifacts/release/real-device/<run-id>/diagnostics.json
```

## 4. Recording results

Append one row per C-step to the matrix at the bottom of
`AVD-WEBGL-ENVIRONMENT-NOTE.md`. Each row must include:

- Device model + OS version (e.g. "Pixel 8, Android 16" / "iPhone 15,
  iOS 18.4").
- Browser / PWA mode (e.g. "Chrome stable", "Safari PWA standalone").
- Marker string + screencap file path + `host-trace-final.log` path.
- `diagnostics.json` path (sanitized).
- VERIFIED / PARTIAL / NOT_RUN verdict per dimension.

## 5. Constraint

Until the matrix has at least one row with `VERIFIED` for every
dimension (SPA auth, shell input, terminal paint, scrollback,
connection-loss recovery, IME, PWA, backgrounding on each target OS),
`BETA_READY` stays `NO`.

No row may be marked `VERIFIED` based on AVD evidence — see
`AVD-WEBGL-ENVIRONMENT-NOTE.md`.

While the matrix has any `NOT_RUN` or `PARTIAL` rows, **no new feature
work may modify renderer / transport / kernel / host wire protocol /
Tauri native runtime in a way that has not already passed the desktop
browser matrix.** Already-passing desktop paths may still receive
defect fixes; nothing new may be built on top until real-device
acceptance clears.