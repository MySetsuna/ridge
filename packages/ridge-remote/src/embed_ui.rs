//! 内嵌前端 SPA 产物（`embed-ui` feature）：手机轻量 SPA + 桌面完整 SPA。
//!
//! **为何存在**：`rdg` 以**单个可执行文件**分发（Release 资产 `rdg-X.Y.Z-*`）。
//! serve 的磁盘探测（[`crate::serve`] 的 `probe_ui_dir`）只找 CWD / exe 上溯里的
//! `static/remote` 与 `web-remote-dist`——单文件场景下哪都没有：
//!   - 手机产物缺失 → LAN 远控恒返回 `REMOTE_UI_MISSING`（iter-61 已修）；
//!   - 桌面产物缺失 → `wants_desktop_ui` 的「产物存在」这一腿恒 false，于是
//!     **桌面浏览器也被发手机 SPA**（iter-62 用户实测：rdg LAN 远控用电脑浏览器
//!     打开，进的是手机端接入页）。
//!
//! 打开 `embed-ui` 后，两份产物都在**编译期**塞进二进制，磁盘探测未命中时回落到
//! 这里。优先级仍是「磁盘 > 内嵌」：本地开发改前端立刻生效（无需重编 Rust），
//! 发布出去的单文件则永远自带两套 UI，桌面/手机各得其所。

/// 请求应命中的 UI 形态。决定内嵌回落取哪一份产物。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiKind {
    /// `static/remote`（`pnpm build:remote`）——手机/触屏轻量 SPA。
    Mobile,
    /// `web-remote-dist`（`pnpm build:desktop-web`）——桌面完整 SPA。
    Desktop,
}

#[cfg(feature = "embed-ui")]
mod imp {
    use rust_embed::RustEmbed;

    /// `static/remote`（`pnpm build:remote` 产物）。目录由 build.rs 保证存在
    /// （可能为空 → `get` 恒 None → 行为与未开 feature 一致）。
    #[derive(RustEmbed)]
    #[folder = "$CARGO_MANIFEST_DIR/../../static/remote"]
    struct MobileUi;

    /// `web-remote-dist`（`pnpm build:desktop-web` 产物）。
    ///
    /// 排除的是**纯冗余**：`remote/` 是 adapter-static 把 `static/remote` 原样拷进
    /// 产物的副本（已由 `MobileUi` 内嵌一份）、`mobile/` 同理、`docs/` 与首页配图
    /// 只服务于官网壳。少嵌 ~3.5 MiB，且桌面 SPA 运行期一个都不取。
    #[derive(RustEmbed)]
    #[folder = "$CARGO_MANIFEST_DIR/../../web-remote-dist"]
    #[exclude = "remote/*"]
    #[exclude = "mobile/*"]
    #[exclude = "docs/*"]
    #[exclude = "*.jpg"]
    struct DesktopUi;

    /// 取一个内嵌文件（相对各自产物根的路径，`/` 分隔）。
    pub fn get_kind(kind: super::UiKind, rel: &str) -> Option<Vec<u8>> {
        match kind {
            super::UiKind::Mobile => MobileUi::get(rel),
            super::UiKind::Desktop => DesktopUi::get(rel),
        }
        .map(|f| f.data.into_owned())
    }

    /// 该形态是否真带了 UI（空目录编译 = 无 index.html）。
    pub fn has_kind(kind: super::UiKind) -> bool {
        get_kind(kind, "index.html").is_some()
    }
}

#[cfg(not(feature = "embed-ui"))]
mod imp {
    pub fn get_kind(_kind: super::UiKind, _rel: &str) -> Option<Vec<u8>> {
        None
    }
    pub fn has_kind(_kind: super::UiKind) -> bool {
        false
    }
}

pub use imp::{get_kind, has_kind};

/// 取手机 SPA 的内嵌文件（旧调用点保留的薄封装）。
pub fn get(rel: &str) -> Option<Vec<u8>> {
    get_kind(UiKind::Mobile, rel)
}

/// 是否真带了手机 SPA。
pub fn has_ui() -> bool {
    has_kind(UiKind::Mobile)
}
