use anyhow::Result;
use clap::Parser;

const VERSION: &str = match option_env!("RIDGE_KERNEL_BUNDLE_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

#[derive(Parser, Debug)]
#[command(
    name = "ridge-kernel",
    version = VERSION,
    about = "Ridge kernel control plane + domain"
)]
struct Args {
    /// Shared launcher flag, also understood by embedded CLI/desktop hosts.
    #[arg(long = "ridge-kernel-host", hide = true)]
    _kernel_host: bool,
    #[arg(long, hide = true, conflicts_with = "installer_parent")]
    installer_check: bool,
    #[arg(long, hide = true, requires_all = ["installer_status", "installer_release"])]
    installer_parent: Option<u32>,
    #[arg(long, hide = true, requires = "installer_parent")]
    installer_status: Option<std::path::PathBuf>,
    #[arg(long, hide = true, requires = "installer_parent")]
    installer_release: Option<std::path::PathBuf>,
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    #[arg(long, default_value_t = 0)]
    port: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let args = Args::parse();
    if args.installer_check { return ridge_kernel::installer::check(); }
    if let Some(parent) = args.installer_parent {
        return ridge_kernel::installer::hold(parent,
            args.installer_status.as_deref().expect("required installer status"),
            args.installer_release.as_deref().expect("required installer release"));
    }
    ridge_kernel::server::run(&args.host, args.port).await
}
