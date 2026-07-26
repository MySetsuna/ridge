//! 内嵌移动 SPA 产物（`embed-ui` feature）。
//!
//! **为何存在**：`rdg` 以**单个可执行文件**分发（Release 资产 `rdg-X.Y.Z-*`）。
//! serve 的磁盘探测（[`crate::serve`] 的 `probe_ui_dir`）只找 CWD / exe 上溯里的
//! `static/remote`——单文件场景下哪都没有，于是 LAN 远控恒返回
//! `REMOTE_UI_MISSING`（用户实测：rdg 开 LAN remote 打开即报错）。
//!
//! 打开 `embed-ui` 后，`pnpm build:remote` 的产物在**编译期**被塞进二进制，
//! 磁盘探测未命中时回落到这里。优先级仍是「磁盘 > 内嵌」：本地开发改前端
//! 立刻生效（无需重编 Rust），发布出去的单文件则永远自带一份 UI。

#[cfg(feature = "embed-ui")]
mod imp {
    use rust_embed::RustEmbed;

    /// `static/remote`（`pnpm build:remote` 产物）。目录由 build.rs 保证存在
    /// （可能为空 → `get` 恒 None → 行为与未开 feature 一致）。
    #[derive(RustEmbed)]
    #[folder = "$CARGO_MANIFEST_DIR/../../static/remote"]
    struct MobileUi;

    /// 取一个内嵌文件（相对 `static/remote` 的路径，`/` 分隔）。
    pub fn get(rel: &str) -> Option<Vec<u8>> {
        MobileUi::get(rel).map(|f| f.data.into_owned())
    }

    /// 是否真带了 UI（空目录编译 = 无 index.html）。
    pub fn has_ui() -> bool {
        MobileUi::get("index.html").is_some()
    }
}

#[cfg(not(feature = "embed-ui"))]
mod imp {
    pub fn get(_rel: &str) -> Option<Vec<u8>> {
        None
    }
    pub fn has_ui() -> bool {
        false
    }
}

pub use imp::{get, has_ui};
