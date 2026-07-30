use anyhow::Result;
use clap::Parser;

const VERSION: &str = match option_env!("RIDGE_MCP_BUNDLE_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

#[derive(Parser)]
#[command(name = "ridge-mcp", version = VERSION, about = "Ridge desktop MCP stdio bridge")]
struct Args {
    /// Print a paste-ready stdio MCP configuration. Never includes endpoint or token.
    #[arg(long)]
    print_config: bool,
    /// Ridge teammate server base URL. Omit to use the current Ridge pane or sidecar.
    #[arg(long)]
    url: Option<String>,
    /// Ridge teammate token. Omit to use the current Ridge pane or sidecar.
    #[arg(long)]
    token: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    if args.print_config {
        println!(
            "{}",
            ridge_mcp_bridge::stdio_config(&std::env::current_exe()?)
        );
        return Ok(());
    }
    ridge_mcp_bridge::run(args.url, args.token).await
}
