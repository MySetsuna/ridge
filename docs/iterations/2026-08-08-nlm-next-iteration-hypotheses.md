# NLM next-iteration hypotheses - 2026-08-08

NLM was queried read-only after the local E2E pass. Its answer is advisory;
the notebook mixes older iteration numbers and claims that are not current
local evidence. No requirement was created or changed.

| Candidate | Minimum evidence | Classification |
|---|---|---|
| PWA background/resume may lose auth state or E2EE reconnect state | Physical iOS/Android PWA, background/airplane-mode soak, then reconnect without unexpected TOTP | `out_of_date_or_unverified` - no physical device evidence |
| Multi-window workspace claim may race PTY teardown and first paint | Two real desktop windows, high PTY/SCM load, claim handoff; assert bounded RPC cancellation and correct first grid | `hypothesis` - local single-window CDP only |
| Cross-volume Explorer move with one locked file may drift source/target cache | Locked-file partial move; source retains failure, target excludes it, clipboard retains exact failed path | `hypothesis` - no targeted physical-volume run |
| High-latency/TURN resize may desynchronise canvas, kernel geometry and PTY | Forced TURN/public path; 10 rapid resizes; compare canvas rect, kernel rows/cols and PTY metadata | `out_of_date_or_unverified` - no TURN or public soak |
| Missing current Sonar authentication can hide new quality regressions | Restore authorized local scanner credentials; rerun final project scan and inspect Quality Gate | `confirmed_by_local_evidence`: local server UP, CE SUCCESS, Quality Gate OK, new coverage 80.2%, new violations 0; notebook historical claims remain unverified |

The NLM response also mentioned a mobile Chrome Messaging error. CodeGraph and
the local source audit did not establish that the project owns such an API, so
it remains a clean-profile/extension-isolation hypothesis and must not trigger
a code change without evidence.

Next iteration entry contract: pick one hypothesis, define the smallest local
or user-authorized device evidence, then update `REQUIREMENTS-SPEC.md` only if
the user explicitly approves a requirement change. Publication and Remote/
Cloud activation remain user-authorized operations.

## Post-E2E read-only NLM loop

After the final local E2E pass, a second read-only query returned these
additional candidates. They are not local bug confirmations and caused no
requirements, source, note, or code write:

| Candidate | Minimum evidence | Classification |
|---|---|---|
| High-frequency Agent PTY fallback injection collides with foreground output | Trace an `agent_send` payload interleaved with TUI/ANSI output while the target is non-idle | `hypothesis` - no local corruption trace |
| DPR change produces WebGPU glyph-atlas shimmer | Same terminal content screenshots at 100/125/150% across monitor changes | `hypothesis` - no physical raster comparison |
| Cross-window workspace claim leaves background pane traffic alive | Two real windows plus RpcClient traffic for a pane absent from the current SSOT | `hypothesis` - local single-window leak trace passed |
| Mobile backgrounding evicts PWA auth/E2EE state | 30+ minute iOS/Android background soak with reconnect logs | `out_of_date_or_unverified` - no physical-device evidence |
| Windows partial-cut permission failure desynchronizes Explorer cache | Restricted-volume partial move, then compare disk, source/target trees, and failed clipboard paths | `hypothesis` - no targeted physical-volume run |

Next iteration therefore remains evidence-first: choose one candidate, obtain the
smallest local or user-authorized trace, and only then change requirements or
code. Publication and Remote/Cloud activation remain user-authorized.
