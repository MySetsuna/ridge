use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
#[command(name = "ridge-mcp", about = "Ridge desktop MCP stdio bridge")]
struct Args {
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
    ridge_mcp_bridge::run(args.url, args.token).await
}
