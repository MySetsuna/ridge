//! 共享工作区 / pane 领域模型（D11：共享实体图谱 + 每连接视图）。
//!
//! 这是 unified-remote 的 **D11** 落地（见
//! `docs/plans/d11-workspace-graph-pane-decoupling-design.md`）。按"杠铃"分波：
//!
//! - **Wave A（本模块当前内容）**：[`pane_tree`]（分屏布局树）+ [`mode`]（pane 模式）
//!   从桌面 `src-tauri` 移入，纯数据结构、零 Tauri、`.ridge` serde 逐字兼容；桌面
//!   经 `engine::pane_tree` / `types::PaneMode` re-export 委托，行为零变化。
//! - **Wave B（类型已建 [`graph`]）**：[`graph::WorkspaceGraph`]（workspaces 集合 +
//!   每 pane 锁定尺寸 + pane CRUD）——cli/cloud host 的工作区/pane 存储（P4 采用，
//!   随 S3）。**纯、不自发事件**：host 在**释放锁之后**经 `Ctx::EventScope::Broadcast`
//!   广播结构变更（保留"先解锁再发"安全序）。桌面**不**改用它作存储（已有 `AppState`，
//!   且 pane 复用已在 Wave A 经共享 [`pane_tree::PaneTree`] 达成）；若 host 采用，须
//!   **持在既有同一把 `workspaces` 写锁内、非独立第二把锁**（否则重开 `pty_generation`
//!   竞态 + 锁倒置）。
//! - **Wave C（随 S4/S5，未建）**：`ViewState`/`ViewRegistry`（每连接视图：active
//!   workspace / focused pane / scroll / selection / 未落盘 buffer / theme，keyed by
//!   [`crate::ctx::ConnectionId`]）。
//!
//! 范围边界：图谱只持 **pane 身份 + 布局 + 共享属性**；`PtyHandle`/`PtyBridge`
//! 句柄、teammate 生命周期、`pty_generation`、scrollback 等留各 host 旁表，按同一
//! pane id 对齐（详设计文档 §1.5 / §10）。

pub mod graph;
pub mod mode;
pub mod pane_tree;
