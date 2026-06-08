//! rdg — Ridge 无头远控 host（面向无图形界面的 Linux/VPS）。可执行名 `rdg`。
//!
//! 用法：
//!   rdg remote --enable    设备码配对，持久化 device JWT
//!   rdg remote --daemon    后台运行，等 controller 接入并桥接 PTY
//!
//! 架构：设备码流(§4.4) → device JWT 持久化(§3) → 信令 WS(§5, role=host) →
//!       WebRTC answerer(§0) → DataChannel 上叠 X25519+ChaCha20Poly1305(§7) →
//!       16ms 攒批的 PTY 桥（复用 portable-pty + fs 搜索/树）。
//!
//! 线协议（统一远控 S3 / 契约 §11.1）：controller↔host **收敛到桌面 host 同款** mux
//! + JSON-RPC 2.0，于是同一个浏览器 controller（`cloudControllerBoot`）既驱动桌面端
//! 也驱动本 cli host。入站 E2EE 明文按 1 字节通道前缀 demux（`mux.rs`）：通道 0x11
//! 是 JSON-RPC 请求/通知（`rpc.rs` 路由到 PTY / fs），通道 0x12 是 TOTP 控制帧
//! （`protocol.rs`）。出站 PTY 字节走 0x10 PANE_RAW（带 paneId），JSON-RPC 响应走
//! 0x11，TOTP 结果走 0x12。cli 经 `$/hello` 只公告 terminal/pty + fs(search/tree)
//! 能力，controller 据此优雅灰掉 IDE 面板（git/workspace/theme/invoke）。

mod batching;
mod config;
mod core_host;
mod daemon;
mod daemon_ctl;
mod device_flow;
mod e2ee;
mod envelope;
mod fs_reuse;
mod ice;
mod login_flow;
mod mux;
mod protocol;
mod pty;
mod rpc;
mod rtc;
mod session;
mod signaling;
mod totp;
mod tui;

use std::io::IsTerminal;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "rdg",
    version,
    about = "Ridge headless remote host for Linux/VPS"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// 交互式 TUI（默认）：在终端里跑一个可交互会话。无子命令时即进入此模式。
    /// 本轮承载本地 shell（passthrough）；LAN/公网控制端将接入同一界面（见
    /// docs/plans/rdg-interactive-tui-and-lan.md）。
    Tui(TuiArgs),

    /// 账号密码登录并直接激活本机：邮箱+密码登录 → （按需设用户名）→ 绑定设备，
    /// 拿到 device 凭据写入 ~/.config/ridge/auth.json。免去设备码 + 浏览器回环。
    /// 带 `--daemon` 时激活后直接进入守护。
    Login(LoginArgs),

    /// 作为**控制端**连接桌面 LAN host（E4）：WS + 自签 TLS，订阅 pane 后
    /// passthrough 进交互式 TUI（与本地 shell 同一界面）。鉴权用 `--code <TOTP>`
    /// （桌面"远程控制"面板显示）或 `--token <session>`。
    Connect(ConnectArgs),

    /// 在本机托管无头 tmux 会话引擎（teammate 协议子集，复用桌面同款 `ridge-tmux`），
    /// 供 PATH 上的 `tmux` shim 连接——让无头会话直接在本 host 运行。
    Tmux(TmuxArgs),
}

#[derive(Args)]
struct TuiArgs {
    /// 指定要拉起的 shell（默认按平台探测）。
    #[arg(long)]
    shell: Option<String>,

    /// 会话 shell 的工作目录（默认 $HOME / 当前目录）。
    #[arg(long)]
    cwd: Option<String>,
}

#[derive(Args)]
struct LoginArgs {
    /// 激活成功后直接进入守护（等价于随后再跑 `rdg remote --daemon`）。
    #[arg(long)]
    daemon: bool,

    /// 指定要拉起的 shell（仅在 --daemon 时生效）。
    #[arg(long)]
    shell: Option<String>,

    /// 会话 shell 的工作目录（仅在 --daemon 时生效）。
    #[arg(long)]
    cwd: Option<String>,

    /// fs 服务根沙箱（仅在 --daemon 时生效，见 `remote --daemon` 的 --root）。
    #[arg(long, env = "RIDGE_REMOTE_ROOT")]
    root: Option<String>,
}

#[derive(Args)]
struct ConnectArgs {
    /// 目标 host：`ip` 或 `ip:port`（缺省端口 9527）。
    host: String,

    /// TOTP 一次性码（桌面"远程控制"面板显示）。`--code` 与 `--token` 二选一。
    #[arg(long)]
    code: Option<String>,

    /// 已配对的会话 token（替代 TOTP）。
    #[arg(long)]
    token: Option<String>,

    /// 不进交互 TUI，跑一次无头协议自检（连接→订阅→回显校验）后退出。
    /// 用于在非 TTY 环境对真实桌面 host 验证驱动（TLS/握手/帧）。
    #[arg(long)]
    probe: bool,

    /// probe 模式收集输出的秒数。
    #[arg(long, default_value_t = 5)]
    probe_seconds: u64,
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
        Some(Command::Tui(args)) => tui::run_local(args.shell, args.cwd).await,
        Some(Command::Login(args)) => run_login(args).await,
        Some(Command::Connect(args)) => {
            if args.probe {
                tui::run_lan_probe(args.host, args.code, args.token, args.probe_seconds).await
            } else {
                tui::run_lan(args.host, args.code, args.token).await
            }
        }
        Some(Command::Tmux(args)) => run_tmux(args).await,
        // 无子命令：进入仪表盘（daemon status + 操作菜单）。
        // 通过菜单的 "Local shell session" 或子命令 `rdg tui` 进入 passthrough TUI。
        None => {
            if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
                tui::dashboard::run().await
            } else {
                eprintln!(
                    "用法：rdg [tui|login|remote|connect|tmux]。无子命令时在交互终端进入仪表盘。\n详见 `rdg --help`。"
                );
                Ok(())
            }
        }
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

/// 账号密码登录 + 自助激活本机（替代设备码浏览器回环）。成功后写入设备凭据；
/// 带 `--daemon` 则直接进入守护。
async fn run_login(args: LoginArgs) -> Result<()> {
    let client = reqwest::Client::builder().build()?;
    let auth = login_flow::run_login(&client).await?;
    if args.daemon {
        tracing::info!(target: "ridge_cli", device = %auth.device_name, "activation complete; entering daemon");
        return daemon::run(args.shell, args.cwd, args.root).await;
    }
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
