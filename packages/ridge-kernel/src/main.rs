use anyhow::Result;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "ridge-kernel",
    version,
    about = "Ridge kernel control plane + domain"
)]
struct Args {
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
    ridge_kernel::server::run(&args.host, args.port).await
}
