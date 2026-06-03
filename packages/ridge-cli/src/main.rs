//! ridge-cli — Ridge 无头远控 host（面向无图形界面的 Linux/VPS）。
//!
//! 用法：
//!   ridge-cli remote --enable    设备码配对，持久化 device JWT
//!   ridge-cli remote --daemon    后台运行，等 controller 接入并桥接 PTY
//!
//! 架构：设备码流(§4.4) → device JWT 持久化(§3) → 信令 WS(§5, role=host) →
//!       WebRTC answerer(§0) → DataChannel 上叠 X25519+ChaCha20Poly1305(§7) →
//!       16ms 攒批的 PTY 桥（复用 portable-pty + fs 搜索/树）。

mod batching;
mod config;
mod daemon;
mod device_flow;
mod e2ee;
mod envelope;
mod fs_reuse;
mod ice;
mod protocol;
mod pty;
mod rtc;
mod session;
mod signaling;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "ridge-cli",
    version,
    about = "Ridge headless remote host for Linux/VPS"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 远程控制：配对（--enable）或后台守护（--daemon）。
    Remote(RemoteArgs),
}

#[derive(Args)]
struct RemoteArgs {
    /// 启动设备码配对流程，绑定后把 device JWT 写入 ~/.config/ridge/auth.json。
    #[arg(long)]
    enable: bool,

    /// 以守护进程运行：连接信令、等待 controller、桥接本地 shell。
    #[arg(long)]
    daemon: bool,

    /// 指定要拉起的 shell（默认按平台探测：$SHELL→bash→sh）。
    #[arg(long)]
    shell: Option<String>,

    /// 会话 shell 的工作目录（默认 $HOME / 当前目录）。
    #[arg(long)]
    cwd: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let cli = Cli::parse();
    match cli.command {
        Command::Remote(args) => run_remote(args).await,
    }
}

async fn run_remote(args: RemoteArgs) -> Result<()> {
    if args.enable {
        let client = reqwest::Client::builder().build()?;
        let auth = device_flow::run_enable(&client).await?;
        // --enable 同时带 --daemon 时，配对成功后直接进入守护。
        if args.daemon {
            tracing::info!(target: "ridge_cli", device = %auth.device_name, "pairing complete; entering daemon");
            return daemon::run(args.shell, args.cwd).await;
        }
        eprintln!("配对完成。运行 `ridge-cli remote --daemon` 开始守护。");
        return Ok(());
    }

    if args.daemon {
        return daemon::run(args.shell, args.cwd).await;
    }

    // 既不 --enable 也不 --daemon：打印用法。
    eprintln!("请指定 --enable（配对）或 --daemon（守护）。详见 `ridge-cli remote --help`。");
    Ok(())
}

/// 初始化日志。`RUST_LOG` 控制级别，默认 info。守护场景下输出走 stderr，
/// systemd 会把它收进 journald。
fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,ridge_cli=info"));
    fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false)
        .init();
}
