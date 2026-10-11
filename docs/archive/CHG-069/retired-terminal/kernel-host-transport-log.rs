/// RIDGE_RTP1_KERNEL=1 routes the per-pane subscribe loop through the
/// RTP1-over-WebSocket kernel client (SPEC-L2-PROTO-001) instead of the
/// bounded-seq-v1 HTTP lease API. Default off so existing shells keep
/// working until they migrate.
fn rtp1_kernel_enabled() -> bool {
    matches!(
        std::env::var("RIDGE_RTP1_KERNEL").ok().as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

/// P2-B8: surface the active mode once at startup so operators can
/// verify the kernel transport without strace'ing the kernel binary.
/// Always prints; the legacy line indicates the bounded-seq-v1 path.
pub fn log_kernel_transport_mode() {
    if rtp1_kernel_enabled() {
        tracing::info!(
            target: "ridge_cli::rtp1_kernel",
            "kernel transport: RTP1-over-WebSocket (RIDGE_RTP1_KERNEL=1); \
             bounded-seq-v1 HTTP path retained as fallback adapter"
        );
        eprintln!("ridge-cli: kernel transport = RTP1-over-WebSocket");
    } else {
        tracing::info!(
            target: "ridge_cli::kernel_host_impl",
            "kernel transport: bounded-seq-v1 HTTP (legacy adapter); \
             set RIDGE_RTP1_KERNEL=1 to opt into RTP1"
        );
    }
}

