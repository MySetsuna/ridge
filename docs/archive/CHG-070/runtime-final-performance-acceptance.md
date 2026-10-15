# Final-build local performance receipt

The rebuilt desktop, CLI and Runtime passed the paired diagnostic OFF/ON comparison. Each setting used the same executable/WASM hashes and unchanged frontend sources, 32 actual child-reply samples for each of three clients across seven scenarios: idle, sustained output, desktop minimized, switch, history, reconnect and Host disconnected.

Total: 1,344 formal samples, zero timeouts, zero failures; warmup samples excluded. The two complete lifecycle receipts also retain original Runtime/PTY identities across forced desktop death, verify CLI/LAN input after exit, and reopen the real desktop onto the original pane.

Comparison with per-scenario P50/P95/P99/max, stage timings, diagnostic data and artifact hashes: `D:\ridge-070-acceptance\temp\ridge-performance-repeated-1E8wlc\comparison.json`. OFF receipt: `..\ridge-desktop-lifecycle-JLd4nP\result.json`; ON receipt: `..\ridge-desktop-lifecycle-agsJkj\result.json`. Log: `.webview2-dev-cdp/070-final-performance.log`.

One run per setting; local debug builds and loopback. Injection, observation and uncontrolled system/GPU scheduling are included. Differences describe these runs; they do not establish causal diagnostic overhead, improvement over an earlier build, WAN/cellular performance or a release-build latency threshold. Actual user-started external Agent acceptance and product installation/upgrade remain separate outstanding receipts. CHG-070 is not marked complete by this measurement.
