//! 内嵌统一 Remote 产物（`embed-ui` feature）：手机轻量 SPA + 桌面完整 SPA。
//!
//! **为何存在**：`rdg` 以**单个可执行文件**分发（Release 资产 `rdg-X.Y.Z-*`）。
//! serve 的磁盘探测（[`crate::serve`] 的 `probe_ui_dir`）只找 CWD / exe 上溯里的
//! `remote-dist`——单文件场景下磁盘目录不存在：
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
    /// `remote-dist/mobile`——手机/触屏轻量 SPA。
    Mobile,
    /// `remote-dist/desktop`——桌面完整 SPA。
    Desktop,
}

impl UiKind {
    pub(crate) const fn dir_name(self) -> &'static str {
        match self {
            Self::Mobile => "mobile",
            Self::Desktop => "desktop",
        }
    }
}

#[cfg(feature = "embed-ui")]
mod imp {
    use rust_embed::RustEmbed;

    /// 单一 `remote-dist` 根；目录由 build.rs 保证存在（可能为空）。
    #[derive(RustEmbed)]
    #[folder = "$CARGO_MANIFEST_DIR/../../remote-dist"]
    #[exclude = "desktop/docs/*"]
    #[exclude = "desktop/*.jpg"]
    struct RemoteUi;

    /// 取一个内嵌文件（相对所选形态根的路径，`/` 分隔）。
    pub fn get_kind(kind: super::UiKind, rel: &str) -> Option<Vec<u8>> {
        RemoteUi::get(&format!("{}/{rel}", kind.dir_name())).map(|f| f.data.into_owned())
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
