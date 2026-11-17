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
    #[arg(long, hide = true)]
    agent_console: Option<String>,
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
    #[arg(long, hide = true, requires = "installer_parent")]
    installer_msi: bool,
    #[arg(long, hide = true, value_parser = ["reserve", "verify", "release"],
        requires_all = ["installer_product", "installer_user"], conflicts_with_all = ["installer_parent", "installer_check"])]
    installer_msi_action: Option<String>,
    #[arg(long, hide = true)]
    installer_product: Option<String>,
    #[arg(long, hide = true, requires = "installer_msi_action")]
    installer_user: Option<String>,
    #[arg(long, hide = true, requires = "installer_msi_action")]
    installer_upgrading: Option<String>,
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
    if let Some(config) = args.agent_console {
        return ridge_kernel::managed_console::run(serde_json::from_str(&config)?).await;
    }
    if args.installer_check { return ridge_kernel::installer::check(); }
    if let Some(action) = args.installer_msi_action {
        return ridge_kernel::installer::msi_action(&action,
            args.installer_product.as_deref().expect("required installer product"),
            args.installer_user.as_deref().expect("required installer user"), args.installer_upgrading.as_deref().unwrap_or(""));
    }
    if let Some(parent) = args.installer_parent {
        let status = args.installer_status.as_deref().expect("required installer status");
        let release = args.installer_release.as_deref().expect("required installer release");
        if args.installer_msi && args.installer_product.is_some() {
            let result = ridge_kernel::installer::hold_msi_product(parent, status, release, args.installer_product.as_deref());
            if result.is_ok() { ridge_kernel::installer::clean_msi_handshake(status, release); }
            return result;
        }
        let hold = if args.installer_msi { ridge_kernel::installer::hold_msi } else { ridge_kernel::installer::hold };
        return hold(parent, status, release);
    }
    if args.installer_product.is_some() { anyhow::bail!("installer product requires an MSI action or reservation"); }
    ridge_kernel::server::run(&args.host, args.port).await
}
