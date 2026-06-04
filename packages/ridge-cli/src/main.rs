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
mod core_host;
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

    /// 在本机托管无头 tmux 会话引擎（teammate 协议子集，复用桌面同款 `ridge-tmux`），
    /// 供 PATH 上的 `tmux` shim 连接——让无头会话直接在本 host 运行。
    Tmux(TmuxArgs),
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

#[derive(Args)]
struct TmuxArgs {
    /// 监听端口（默认 0 = 由系统分配，启动后打印实际端口）。
    /// 可由 RIDGE_TMUX_PORT 提供（命令行 > 环境变量 > 默认）。
    #[arg(long, env = "RIDGE_TMUX_PORT", default_value_t = 0)]
    port: u16,

    /// 监听地址（默认仅本机回环；跨机访问需自行加固，引擎无沙箱）。
    /// 可由 RIDGE_TMUX_BIND 提供（命令行 > 环境变量 > 默认）。
    #[arg(long, env = "RIDGE_TMUX_BIND", default_value = "127.0.0.1")]
    bind: String,

    /// 鉴权 token（默认随机生成并打印；`tmux` shim 经 RIDGE_TEAMMATE_TOKEN 读取）。
    /// 可由 RIDGE_TMUX_TOKEN 提供——systemd EnvironmentFile 比命令行更安全
    /// （命令行上的 token 会被 `ps` 看到）。
    #[arg(long, env = "RIDGE_TMUX_TOKEN")]
    token: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let cli = Cli::parse();
    match cli.command {
        Command::Remote(args) => run_remote(args).await,
        Command::Tmux(args) => run_tmux(args).await,
    }
}

/// 托管无头 tmux 引擎：绑定本机 HTTP 端点，打印供 agent 注入的 `RIDGE_TEAMMATE_*`
/// env，然后服务 `ridge-tmux` 的 native 路由直到进程退出。引擎与桌面端逐字节同源。
async fn run_tmux(args: TmuxArgs) -> Result<()> {
    use anyhow::Context as _;

    // 空/全空白的 token（如 EnvironmentFile 里 `RIDGE_TMUX_TOKEN=` 占位行）视为未提供，
    // 退回随机生成——避免静默起一个无鉴权效果的空 token。
    let token = args
        .token
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(gen_token);
    let addr = format!("{}:{}", args.bind, args.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("bind {addr}"))?;
    let local = listener.local_addr().context("read local addr")?;
    let url = format!("http://{local}");

    // 这些导出行供 agent 进程注入；`tmux` shim 读 RIDGE_TEAMMATE_URL / _TOKEN。
    // 写 stderr，与本 crate 其余日志一致（systemd → journald）。
    eprintln!("ridge-cli tmux engine listening on {url}");
    eprintln!();
    eprintln!("  export RIDGE_TEAMMATE_URL={url}");
    eprintln!("  export RIDGE_TEAMMATE_TOKEN={token}");
    eprintln!();
    eprintln!("将 `tmux` shim 放入 PATH 后，本 host 上的 agent 即可创建无头 tmux 会话。");

    let ctx = ridge_tmux::http::NativeHttpCtx::headless(token);
    ridge_tmux::http::serve(listener, ctx)
        .await
        .context("tmux engine server stopped")?;
    Ok(())
}

/// 32 个十六进制字符的随机 token（rand 0.8，已是本 crate 依赖）。
fn gen_token() -> String {
    use rand::Rng as _;
    let mut rng = rand::thread_rng();
    (0..32)
        .map(|_| {
            let nibble: u8 = rng.gen_range(0..16);
            std::char::from_digit(nibble as u32, 16).unwrap_or('0')
        })
        .collect()
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
