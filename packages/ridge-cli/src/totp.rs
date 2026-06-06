//! 云远控二次验证：RFC 6238 TOTP（契约 §4）。
//!
//! 实现已统一下沉到 `ridge_core::totp`（桌面 / cli / 未来 LAN host 共用一份权威实现，
//! 见 docs/plans/rdg-interactive-tui-and-lan.md §E1）。本模块仅再导出，保持
//! `crate::totp::RemoteTotp` 调用点（`session.rs`）不变。
pub use ridge_core::totp::RemoteTotp;
