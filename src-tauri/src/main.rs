// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if ridge_kernel::client::kernel_host_requested() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "info".into()),
            )
            .try_init();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("build embedded ridge-kernel runtime");
        if let Err(error) = runtime.block_on(ridge_kernel::server::run("127.0.0.1", 0)) {
            tracing::error!(%error, "embedded ridge-kernel host failed");
            std::process::exit(1);
        }
        return;
    }
    ridge_lib::run()
}
