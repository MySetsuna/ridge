//! 进程级日志 & 崩溃捕获。
//!
//! 目标：
//! - 将 panic（主线程 / 后台线程 / 子任务）完整落盘到
//!   `<LOCALAPPDATA>\ridge\logs\crash-YYYY-MM-DD.log`；
//! - `tracing` 事件通过 rolling-daily 文件 appender 输出到
//!   `<LOCALAPPDATA>\ridge\logs\ridge-YYYY-MM-DD.log`，保留 stderr 以便开发。
//!
//! 只在 `init_once()` 第一次调用时真正安装；多次调用不会重复注册 panic hook。

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::Local;
use once_cell::sync::OnceCell;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

static INIT: AtomicBool = AtomicBool::new(false);
static GUARD: OnceCell<WorkerGuard> = OnceCell::new();
static LOG_DIR: OnceCell<PathBuf> = OnceCell::new();

/// 返回日志/崩溃文件的目录，`init_once` 后才可用。
#[allow(dead_code)]
pub fn log_dir() -> Option<&'static PathBuf> {
    LOG_DIR.get()
}

/// 初始化 tracing 订阅 + panic hook。重复调用无副作用。
pub fn init_once(app_data_dir: &PathBuf) {
    if INIT.swap(true, Ordering::SeqCst) {
        return;
    }
    let logs_dir = app_data_dir.join("logs");
    let _ = std::fs::create_dir_all(&logs_dir);
    let _ = LOG_DIR.set(logs_dir.clone());

    // 每日滚动文件：ridge-YYYY-MM-DD.log
    let file_appender = tracing_appender::rolling::daily(&logs_dir, "ridge.log");
    let (nb, guard) = tracing_appender::non_blocking(file_appender);
    let _ = GUARD.set(guard);

    let filter = EnvFilter::try_from_env("WIND_LOG").unwrap_or_else(|_| EnvFilter::new("info"));

    let file_layer = fmt::layer()
        .with_writer(nb)
        .with_ansi(false)
        .with_target(true)
        .with_thread_names(true)
        .with_file(true)
        .with_line_number(true);

    // stderr 层仅在 debug 构建里保留，release 时只进文件避免泄漏噪音。
    #[cfg(debug_assertions)]
    {
        let stderr_layer = fmt::layer()
            .with_writer(std::io::stderr)
            .with_ansi(true)
            .with_target(false);
        let _ = tracing_subscriber::registry()
            .with(filter)
            .with(file_layer)
            .with(stderr_layer)
            .try_init();
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = tracing_subscriber::registry()
            .with(filter)
            .with(file_layer)
            .try_init();
    }

    install_panic_hook(logs_dir);
}

fn install_panic_hook(logs_dir: PathBuf) {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // 先写崩溃文件（单次 panic 专用，便于事故溯源）。
        let stamp = Local::now().format("%Y-%m-%d").to_string();
        let path = logs_dir.join(format!("crash-{stamp}.log"));
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let now = Local::now().to_rfc3339();
            let location = info
                .location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_else(|| "<unknown>".into());
            let payload = info
                .payload()
                .downcast_ref::<&'static str>()
                .copied()
                .map(|s| s.to_string())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "<non-string panic>".into());

            let _ = writeln!(f, "\n===== RIDGE CRASH @ {now} =====");
            let _ = writeln!(f, "location: {location}");
            let _ = writeln!(
                f,
                "thread  : {}",
                std::thread::current().name().unwrap_or("<unnamed>")
            );
            let _ = writeln!(f, "message : {payload}");
            let bt = backtrace::Backtrace::new();
            let _ = writeln!(f, "backtrace:\n{bt:?}");
        }
        // 再让默认 hook 执行（打印到 stderr），最后 tracing 记录一条 error。
        default_hook(info);
        tracing::error!(target: "ridge::panic", panic = %info, "panic captured by hook");
    }));
}

/// 给那些"启动后不允许死"的后台线程用的包装：panic 被 catch_unwind 吞掉、落盘并触发重启回调。
///
/// 返回子线程 JoinHandle；若线程 panic，则调用 `on_panic`（用于调度重启）。
#[allow(dead_code)]
pub fn spawn_supervised<F, R>(name: impl Into<String>, body: F, on_panic: R)
where
    F: FnOnce() + Send + 'static + std::panic::UnwindSafe,
    R: FnOnce() + Send + 'static,
{
    let name_str = name.into();
    let name_for_thread = name_str.clone();
    let _ = std::thread::Builder::new()
        .name(name_str)
        .spawn(move || {
            let result = std::panic::catch_unwind(body);
            if result.is_err() {
                tracing::error!(target: "ridge::supervise", thread = %name_for_thread, "thread panicked; supervisor running fallback");
                on_panic();
            }
        });
}
