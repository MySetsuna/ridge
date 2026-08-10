# Wave 79: RDG LAN CDP warm-up evidence

## Finding

The first fresh `dev:cdp` run reached the RDG LAN host, pane creation, subscription, resize, UUID validation, and ping/pong. The probe still reported `liveFrames=0` and `echoSeen=false` because it sent the first input 800 ms after creating a Windows ConPTY.

This was an E2E timing false negative, not evidence that the Remote output lane was disconnected. The companion PTY parser E2E passed against the same dev instance with 11 binary frames, 3 metadata frames, UTF-8 decoding, OSC title, and OSC cwd validation.

## Change

`scripts/cdp-lan-probe.mjs` now:

- waits 2.5 s before the first input, matching `cdp-pty-parsers.mjs`;
- keeps a bounded 90 s cold-start timeout.

## Fresh evidence

With the new dev binary and `RIDGE_CDP_ALLOW_NON_BREAKAWAY=1` for the constrained Windows Job harness:

- `pnpm cdp:smoke`: PASS;
- `pnpm cdp:pty`: PASS;
- `node scripts/cdp-lan-probe.mjs`: PASS — `liveFrames=4`, `echoSeen=true`, `pong=true`, `uuidMatch=true`.

The same-job flag is a local test-harness accommodation; it is not a production behavior change. Physical iOS/Android keyboard evidence, public/TURN connectivity, third-party runtime, and a fresh Sonar analysis remain external gates.
