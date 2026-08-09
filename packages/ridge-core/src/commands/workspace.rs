//! Workspace 快照 handler —— R0 内核化样板 B（2026-07-11）。
//!
//! 把桌面 `src-tauri/src/commands/ridge_file.rs::snapshot_workspace` 的**纯加工逻辑**
//! （pane 树 JSON + 各 pane cwd 发现 git 仓库根去重 + pane 标题）下沉 ridge-core，
//! 让桌面/rdg 无头两端共用一份。宿主经 [`WorkspaceReader`] 端口提供**原始数据**，
//! core 负责加工——与 [`super::settings`] 的 `HostStateAccessor` 同款「宿主状态经 trait
//! 端口注入」范式。
//!
//! **本刀范围（安全、可离线测）**：core 端 trait + 纯加工 + handler + 单测。dispatch
//! 路由 + 桌面/rdg adapter 接线为下一刀（需真机验证桌面路径，见 R0 spec）。届时
//! [`WorkspaceAccessor`] 与 `settings::HostStateAccessor` 应合并为聚合 host-state trait
//! （一个 `Ctx` 只有一个 state；同时服务两域的宿主需聚合，R0 spec 风险项）。

use std::collections::{BTreeSet, HashMap};

use serde::Serialize;
use serde_json::Value;

use crate::ctx::Ctx;
use crate::error::{CoreError, CoreResult};

/// 宿主提供的**原始**工作区数据（未加工）。桌面从 `AppState.workspaces` 填，rdg 从
/// daemon 状态填。
#[derive(Debug, Clone)]
pub struct WorkspaceRaw {
    /// pane 树的 JSON 快照（宿主已序列化其 `PaneTree`）。
    pub pane_tree: Value,
    /// 各 pane 的 cwd（用于向上发现 git 仓库根）。
    pub pane_cwds: Vec<String>,
    /// pane 标题（teammate 显示名），键 = pane UUID 串。
    pub pane_titles: HashMap<String, String>,
}

/// 工作区列表的一项（`list_workspaces`）。序列化为前端 camelCase 形状，与桌面
/// `commands::workspace::WorkspaceInfo` 逐字一致（id / index / name / displaySeq）。
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceEntry {
    pub id: String,
    pub index: usize,
    pub name: Option<String>,
    pub display_seq: u64,
}

/// 宿主暴露「读工作区」的端口（D4：core 不认桌面 `AppState`）。
pub trait WorkspaceReader: Send + Sync {
    /// 返回该工作区的原始快照数据；工作区不存在 → `None`。
    fn workspace_raw(&self, workspace_id: &str) -> Option<WorkspaceRaw>;
    /// 活动工作区 id 字符串（`get_active_workspace_id`）。
    fn active_workspace(&self) -> String;
    /// 工作区列表快照（`list_workspaces`）：按顺序 + 名 + display_seq。
    fn workspaces_list(&self) -> Vec<WorkspaceEntry>;
    /// 某工作区的 pane 布局（`get_pane_layout[_for]`）序列化为 JSON。纯读、不触 PTY。
    /// 工作区不存在 / id 非法 → `Err`。
    fn pane_layout(&self, workspace_id: &str) -> Result<Value, String>;
    /// pane 的 scrollback 末尾最多 `max_bytes` 字节（`get_pane_scrollback_tail`）序列化为
    /// JSON。纯读**已存字节**（非 live PTY）；pane_id 非法 → `Err`。
    fn pane_scrollback_tail(&self, pane_id: &str, max_bytes: usize) -> Result<Value, String>;
    /// pane 的 scrollback 中 `before_seq` 之前最多 `max_bytes` 字节（`get_pane_scrollback_before`，
    /// 向上翻页）序列化为 JSON。纯读；pane_id 非法 → `Err`。
    fn pane_scrollback_before(
        &self,
        pane_id: &str,
        before_seq: u64,
        max_bytes: usize,
    ) -> Result<Value, String>;
    /// 所有 native tmux 后台会话列表（`list_native_sessions`）序列化为 JSON。全局纯读、
    /// 不触 PTY——供远端运维发现后台 agent 会话。
    fn native_sessions(&self) -> Value;
}

/// 宿主暴露「写工作区元数据」的端口（R0 内核化，写命令下沉起点）。仅覆盖**不触 PTY
/// 生命周期**的纯元数据写（切换活动区等）；建/关/分屏这类会动 PTY 的留在宿主运行时。
pub trait WorkspaceWriter: Send + Sync {
    /// 切换活动工作区。工作区不存在（或 id 非法）→ `Err(消息)`。宿主原子地校验+置位。
    fn set_active_workspace(&self, workspace_id: &str) -> Result<(), String>;
    /// 重排工作区顺序：把 `from_index` 处的工作区移到 `to_index`。越界 → `Err`。
    fn reorder_workspaces(&self, from_index: usize, to_index: usize) -> Result<(), String>;
    /// 重命名工作区。宿主负责落盘 + 向远端/前端广播（不触 PTY）。非法 id → `Err`。
    fn rename_workspace(&self, workspace_id: &str, name: &str) -> Result<(), String>;
    /// 新建根工作区（空终端表，不触 PTY），切为活动区并广播。返回新工作区 id 串。
    fn create_workspace(&self, name: Option<&str>) -> Result<String, String>;
    /// 关闭工作区（从顺序表/映射移除→其终端表随之 drop）。剩最后一个 / 非法 id → `Err`。
    /// **破坏性**：会 drop 该区 PTY 句柄；语义与桌面命令一致（远端亦可触发，允许列表已放行）。
    fn close_workspace(&self, workspace_id: &str) -> Result<(), String>;
    /// 把当前活动工作区存进历史（宿主经 app 数据目录文件 IO）。返回 history id。
    fn save_workspace(&self, name: Option<&str>) -> Result<String, String>;
    /// 把工作区存为 .ridge 存档文件（宿主文件 IO）。名空/非法 id → `Err`。返回文件路径串。
    fn save_workspace_to_file(
        &self,
        workspace_id: &str,
        name: &str,
        path: Option<&str>,
    ) -> Result<String, String>;
    /// 删该工作区关联的 .ridge 存档文件（**破坏性**，仅此文件）。非法 id → `Err`。
    fn delete_workspace_file(&self, workspace_id: &str) -> Result<(), String>;
    /// resize **既有** pane 的 PTY（非 spawn/kill）+ 解析器 wipe/delta。pane/工作区 id
    /// 非法或不存在 → `Err`。`is_alt`/`is_inline_tui` 影响 wipe 顺序（宿主内部再与解析器
    /// 权威值 OR）。
    fn resize_pane(
        &self,
        workspace_id: &str,
        pane_id: &str,
        rows: u16,
        cols: u16,
        is_alt: bool,
        is_inline_tui: bool,
    ) -> Result<(), String>;
    /// 为既有 pane 起 shell PTY（`create_pane`，在活动工作区）。**spawn 子进程**——语义与
    /// 桌面命令一致。pane 非法/不存在或起进程失败 → `Err`。
    fn create_pane(&self, pane_id: &str, shell: Option<&str>) -> Result<(), String>;
    /// 分裂既有 pane（`split_pane`：改 pane 树 + 继承 cwd + **起新 PTY**）。返回分裂结果
    /// （新 pane 等）序列化 JSON。pane 非法/不存在或失败 → `Err`。
    fn split_pane(&self, pane_id: &str, direction: &str) -> Result<Value, String>;
}

/// 加工后的工作区快照（`git_repos`/`pane_titles` 语义与桌面 `snapshot_workspace` 一致）。
#[derive(Debug, Serialize, PartialEq)]
pub struct WorkspaceSnapshot {
    pub pane_tree: Value,
    pub git_repos: Vec<String>,
    pub pane_titles: HashMap<String, String>,
}

/// 纯加工：从原始数据算出快照。`git_repos` = 各 cwd 向上找 `.git` 根、去重 + 排序
/// （`BTreeSet`，与桌面逐字一致）。纯函数、可离线测（不碰宿主状态）。
pub fn build_workspace_snapshot(raw: WorkspaceRaw) -> WorkspaceSnapshot {
    let git_repos: Vec<String> = raw
        .pane_cwds
        .into_iter()
        .filter_map(crate::commands::git::find_git_repo_root)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    WorkspaceSnapshot {
        pane_tree: raw.pane_tree,
        git_repos,
        pane_titles: raw.pane_titles,
    }
}

/// handler：`get_workspace_snapshot`。经 [`WorkspaceAccessor`] 取宿主原始数据 → 加工 →
/// JSON。工作区不存在 → [`CoreError::InvalidArgs`]。
pub fn get_workspace_snapshot(ctx: &Ctx, workspace_id: &str) -> CoreResult<Value> {
    // 经**聚合** accessor 取宿主的 WorkspaceReader（R0：一个 Ctx 一个 state，settings 与
    // workspace 等多域共用 `super::settings::HostStateAccessor`，见 `settings::HostState`）。
    let reader = ctx.state::<super::settings::HostStateAccessor>()?;
    let raw = reader
        .0
        .workspace_raw(workspace_id)
        .ok_or_else(|| CoreError::InvalidArgs(format!("workspace {workspace_id} 不存在")))?;
    serde_json::to_value(build_workspace_snapshot(raw)).map_err(CoreError::internal)
}

/// handler：`get_active_workspace_id`。返回活动工作区 id 字符串（与桌面命令同形）。
pub fn get_active_workspace_id(ctx: &Ctx) -> CoreResult<Value> {
    let reader = ctx.state::<super::settings::HostStateAccessor>()?;
    Ok(Value::String(reader.0.active_workspace()))
}

/// handler：`list_workspaces`。返回工作区列表（camelCase，与桌面 `WorkspaceInfo` 同形）。
pub fn list_workspaces(ctx: &Ctx) -> CoreResult<Value> {
    let reader = ctx.state::<super::settings::HostStateAccessor>()?;
    serde_json::to_value(reader.0.workspaces_list()).map_err(CoreError::internal)
}

/// handler：`get_pane_layout`。返回**活动**工作区的 pane 布局 JSON（与桌面命令同形）。
pub fn get_pane_layout(ctx: &Ctx) -> CoreResult<Value> {
    let reader = ctx.state::<super::settings::HostStateAccessor>()?;
    let wid = reader.0.active_workspace();
    reader.0.pane_layout(&wid).map_err(CoreError::InvalidArgs)
}

/// handler：`get_pane_layout_for`。返回指定工作区的 pane 布局 JSON（§4a keep-alive 预取）。
pub fn get_pane_layout_for(ctx: &Ctx, workspace_id: &str) -> CoreResult<Value> {
    let reader = ctx.state::<super::settings::HostStateAccessor>()?;
    reader
        .0
        .pane_layout(workspace_id)
        .map_err(CoreError::InvalidArgs)
}

/// handler：`get_pane_scrollback_tail`。返回 pane scrollback 末尾块 JSON（纯读已存字节）。
pub fn get_pane_scrollback_tail(ctx: &Ctx, pane_id: &str, max_bytes: usize) -> CoreResult<Value> {
    let reader = ctx.state::<super::settings::HostStateAccessor>()?;
    reader
        .0
        .pane_scrollback_tail(pane_id, max_bytes)
        .map_err(CoreError::InvalidArgs)
}

/// handler：`get_pane_scrollback_before`。返回 pane scrollback 中 before_seq 前的块 JSON（翻页）。
pub fn get_pane_scrollback_before(
    ctx: &Ctx,
    pane_id: &str,
    before_seq: u64,
    max_bytes: usize,
) -> CoreResult<Value> {
    let reader = ctx.state::<super::settings::HostStateAccessor>()?;
    reader
        .0
        .pane_scrollback_before(pane_id, before_seq, max_bytes)
        .map_err(CoreError::InvalidArgs)
}

/// handler：`list_native_sessions`。返回所有 native tmux 后台会话（全局纯读）。
pub fn list_native_sessions(ctx: &Ctx) -> CoreResult<Value> {
    let reader = ctx.state::<super::settings::HostStateAccessor>()?;
    Ok(reader.0.native_sessions())
}

/// handler：`switch_workspace`。切换活动工作区（与桌面命令同语义：校验存在→置活动，
/// 不存在/非法 id → `InvalidArgs`）。经聚合 accessor 的 [`WorkspaceWriter`] 端口写。
pub fn switch_workspace(ctx: &Ctx, workspace_id: &str) -> CoreResult<Value> {
    let writer = ctx.state::<super::settings::HostStateAccessor>()?;
    writer
        .0
        .set_active_workspace(workspace_id)
        .map_err(CoreError::InvalidArgs)?;
    Ok(Value::Null)
}

/// handler：`reorder_workspaces`。把 `fromIndex` 处工作区移到 `toIndex`（与桌面命令同
/// 语义：越界 → `InvalidArgs`）。经 [`WorkspaceWriter`] 端口写。
pub fn reorder_workspaces(ctx: &Ctx, from_index: usize, to_index: usize) -> CoreResult<Value> {
    let writer = ctx.state::<super::settings::HostStateAccessor>()?;
    writer
        .0
        .reorder_workspaces(from_index, to_index)
        .map_err(CoreError::InvalidArgs)?;
    Ok(Value::Null)
}

/// handler：`rename_workspace`。重命名工作区（宿主落盘+广播，与桌面命令同语义）。经
/// [`WorkspaceWriter`] 端口写；非法 id → `InvalidArgs`。
pub fn rename_workspace(ctx: &Ctx, workspace_id: &str, name: &str) -> CoreResult<Value> {
    let writer = ctx.state::<super::settings::HostStateAccessor>()?;
    writer
        .0
        .rename_workspace(workspace_id, name)
        .map_err(CoreError::InvalidArgs)?;
    Ok(Value::Null)
}

/// handler：`create_workspace`。新建根工作区（与桌面命令同语义）。经 [`WorkspaceWriter`]
/// 端口写，返回新工作区 id 串。
pub fn create_workspace(ctx: &Ctx, name: Option<&str>) -> CoreResult<Value> {
    let writer = ctx.state::<super::settings::HostStateAccessor>()?;
    let id = writer
        .0
        .create_workspace(name)
        .map_err(CoreError::internal)?;
    Ok(Value::String(id))
}

/// handler：`close_workspace`。关闭工作区（与桌面命令同语义:剩最后一个/非法 id →
/// `InvalidArgs`）。经 [`WorkspaceWriter`] 端口写（破坏性，drop 该区 PTY）。
pub fn close_workspace(ctx: &Ctx, workspace_id: &str) -> CoreResult<Value> {
    let writer = ctx.state::<super::settings::HostStateAccessor>()?;
    writer
        .0
        .close_workspace(workspace_id)
        .map_err(CoreError::InvalidArgs)?;
    Ok(Value::Null)
}

/// handler：`save_workspace`。存当前工作区到历史（宿主文件 IO）。返回 history id 串。
/// 注：`restore_workspace` **未**入 REMOTE_ALLOWLIST（远端还原工作区属安全面扩张，需显式
/// 授权决策，故不下沉 dispatch）；桌面侧仍走 `commands::workspace::restore_workspace_core`。
pub fn save_workspace(ctx: &Ctx, name: Option<&str>) -> CoreResult<Value> {
    let writer = ctx.state::<super::settings::HostStateAccessor>()?;
    let id = writer.0.save_workspace(name).map_err(CoreError::internal)?;
    Ok(Value::String(id))
}

/// handler：`save_workspace_to_file`。存工作区为 .ridge 文件（宿主文件 IO）。名空 →
/// `InvalidArgs`；其余失败 → `Internal`。返回文件路径串。
pub fn save_workspace_to_file(
    ctx: &Ctx,
    workspace_id: &str,
    name: &str,
    path: Option<&str>,
) -> CoreResult<Value> {
    let writer = ctx.state::<super::settings::HostStateAccessor>()?;
    let p = writer
        .0
        .save_workspace_to_file(workspace_id, name, path)
        .map_err(CoreError::internal)?;
    Ok(Value::String(p))
}

/// handler：`delete_workspace_file`。删工作区关联的 .ridge 文件（破坏性，宿主文件 IO）。
pub fn delete_workspace_file(ctx: &Ctx, workspace_id: &str) -> CoreResult<Value> {
    let writer = ctx.state::<super::settings::HostStateAccessor>()?;
    writer
        .0
        .delete_workspace_file(workspace_id)
        .map_err(CoreError::internal)?;
    Ok(Value::Null)
}

/// handler：`resize_pane`。resize 既有 pane PTY（与桌面命令同语义）。pane 不存在/非法 →
/// `InvalidArgs`。经 [`WorkspaceWriter`] 端口写。
#[allow(clippy::too_many_arguments)]
pub fn resize_pane(
    ctx: &Ctx,
    workspace_id: &str,
    pane_id: &str,
    rows: u16,
    cols: u16,
    is_alt: bool,
    is_inline_tui: bool,
) -> CoreResult<Value> {
    let writer = ctx.state::<super::settings::HostStateAccessor>()?;
    writer
        .0
        .resize_pane(workspace_id, pane_id, rows, cols, is_alt, is_inline_tui)
        .map_err(CoreError::InvalidArgs)?;
    Ok(Value::Null)
}

/// handler：`create_pane`。为既有 pane 起 shell PTY（与桌面命令同语义）。失败 → `Internal`
/// （起进程失败等）。经 [`WorkspaceWriter`] 端口写。
pub fn create_pane(ctx: &Ctx, pane_id: &str, shell: Option<&str>) -> CoreResult<Value> {
    let writer = ctx.state::<super::settings::HostStateAccessor>()?;
    writer
        .0
        .create_pane(pane_id, shell)
        .map_err(CoreError::internal)?;
    Ok(Value::Null)
}

/// handler：`split_pane`。分裂既有 pane（与桌面命令同语义），返回分裂结果 JSON。失败 →
/// `Internal`。经 [`WorkspaceWriter`] 端口写。
pub fn split_pane(ctx: &Ctx, pane_id: &str, direction: &str) -> CoreResult<Value> {
    let writer = ctx.state::<super::settings::HostStateAccessor>()?;
    writer
        .0
        .split_pane(pane_id, direction)
        .map_err(CoreError::internal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::CapabilitySet;
    use crate::commands::settings::HostStateAccessor;
    use crate::ctx::test_support::ctx_with_state;
    use std::sync::Arc;

    fn tmp_git_repo(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let d = std::env::temp_dir().join(format!("ridge_ws_{tag}_{nanos}"));
        std::fs::create_dir_all(d.join(".git")).unwrap();
        d
    }

    #[test]
    fn build_snapshot_dedups_and_sorts_git_repos() {
        let repo = tmp_git_repo("dedup");
        let sub = repo.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let raw = WorkspaceRaw {
            pane_tree: serde_json::json!({ "root": true }),
            // 两个 cwd（repo 与 repo/sub）都归到同一个 .git 根 → 去重后仅 1。
            pane_cwds: vec![
                repo.to_string_lossy().to_string(),
                sub.to_string_lossy().to_string(),
            ],
            pane_titles: HashMap::new(),
        };
        let snap = build_workspace_snapshot(raw);
        assert_eq!(snap.git_repos.len(), 1);
        assert_eq!(snap.pane_tree, serde_json::json!({ "root": true }));
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn build_snapshot_empty_cwds_no_repos() {
        let raw = WorkspaceRaw {
            pane_tree: Value::Null,
            pane_cwds: vec![],
            pane_titles: HashMap::new(),
        };
        assert!(build_workspace_snapshot(raw).git_repos.is_empty());
    }

    struct FakeReader(Option<WorkspaceRaw>);
    impl WorkspaceReader for FakeReader {
        fn workspace_raw(&self, _id: &str) -> Option<WorkspaceRaw> {
            self.0.clone()
        }
        fn active_workspace(&self) -> String {
            "ws-active".to_string()
        }
        fn workspaces_list(&self) -> Vec<WorkspaceEntry> {
            vec![WorkspaceEntry {
                id: "ws-active".into(),
                index: 0,
                name: Some("W".into()),
                display_seq: 1,
            }]
        }
        fn pane_layout(&self, _id: &str) -> Result<Value, String> {
            Ok(Value::Null)
        }
        fn pane_scrollback_tail(&self, _pane: &str, _max: usize) -> Result<Value, String> {
            Ok(Value::Null)
        }
        fn pane_scrollback_before(
            &self,
            _pane: &str,
            _before: u64,
            _max: usize,
        ) -> Result<Value, String> {
            Ok(Value::Null)
        }
        fn native_sessions(&self) -> Value {
            Value::Array(vec![])
        }
    }
    // 聚合 HostState 也要求 UserDefaultCwdStore；本测试只走 workspace 端口 → 空实现。
    impl crate::commands::settings::UserDefaultCwdStore for FakeReader {
        fn set_user_default_cwd(&self, _path: Option<std::path::PathBuf>) {}
    }
    // 聚合 HostState 也要求 WorkspaceWriter；记录/返回可控结果供 switch 测试断言。
    struct FakeWriter {
        exists: bool,
        switched: std::sync::Mutex<Option<String>>,
        reordered: std::sync::Mutex<Option<(usize, usize)>>,
        renamed: std::sync::Mutex<Option<(String, String)>>,
    }
    impl WorkspaceReader for FakeWriter {
        fn workspace_raw(&self, _id: &str) -> Option<WorkspaceRaw> {
            None
        }
        fn active_workspace(&self) -> String {
            String::new()
        }
        fn workspaces_list(&self) -> Vec<WorkspaceEntry> {
            vec![]
        }
        fn pane_layout(&self, id: &str) -> Result<Value, String> {
            // 回带 id 的可断言值（验证 handler 把 workspace_id 透传到端口）。
            Ok(serde_json::json!({ "for": id }))
        }
        fn pane_scrollback_tail(&self, pane: &str, max: usize) -> Result<Value, String> {
            Ok(serde_json::json!({ "tail": pane, "max": max }))
        }
        fn pane_scrollback_before(
            &self,
            pane: &str,
            before: u64,
            max: usize,
        ) -> Result<Value, String> {
            Ok(serde_json::json!({ "pane": pane, "before": before, "max": max }))
        }
        fn native_sessions(&self) -> Value {
            serde_json::json!([{ "name": "bg-sess" }])
        }
    }
    impl crate::commands::settings::UserDefaultCwdStore for FakeWriter {
        fn set_user_default_cwd(&self, _path: Option<std::path::PathBuf>) {}
    }
    impl WorkspaceWriter for FakeWriter {
        fn set_active_workspace(&self, id: &str) -> Result<(), String> {
            if !self.exists {
                return Err(format!("workspace {id} 不存在"));
            }
            *self.switched.lock().unwrap() = Some(id.to_string());
            Ok(())
        }
        fn reorder_workspaces(&self, from: usize, to: usize) -> Result<(), String> {
            if !self.exists {
                return Err("无效的索引".into()); // 复用 exists 作「越界」开关
            }
            *self.reordered.lock().unwrap() = Some((from, to));
            Ok(())
        }
        fn rename_workspace(&self, id: &str, name: &str) -> Result<(), String> {
            if !self.exists {
                return Err(format!("workspace {id} 不存在"));
            }
            *self.renamed.lock().unwrap() = Some((id.to_string(), name.to_string()));
            Ok(())
        }
        fn create_workspace(&self, _name: Option<&str>) -> Result<String, String> {
            Ok("ws-created".to_string())
        }
        fn close_workspace(&self, _id: &str) -> Result<(), String> {
            if !self.exists {
                return Err("无法关闭最后一个工作区".into()); // 复用 exists 作错误开关
            }
            Ok(())
        }
        fn save_workspace(&self, _name: Option<&str>) -> Result<String, String> {
            Ok("hist-1".to_string())
        }
        fn save_workspace_to_file(
            &self,
            _id: &str,
            name: &str,
            _path: Option<&str>,
        ) -> Result<String, String> {
            if name.trim().is_empty() {
                return Err("工作区名不能为空".into());
            }
            Ok(format!("/saved/{name}.ridge"))
        }
        fn delete_workspace_file(&self, _id: &str) -> Result<(), String> {
            Ok(())
        }
        fn resize_pane(
            &self,
            _ws: &str,
            _pane: &str,
            _rows: u16,
            _cols: u16,
            _alt: bool,
            _inline: bool,
        ) -> Result<(), String> {
            if !self.exists {
                return Err("pane 不存在".into()); // 复用 exists 作错误开关
            }
            Ok(())
        }
        fn create_pane(&self, _pane: &str, _shell: Option<&str>) -> Result<(), String> {
            if !self.exists {
                return Err("起进程失败".into());
            }
            Ok(())
        }
        fn split_pane(&self, pane: &str, direction: &str) -> Result<Value, String> {
            if !self.exists {
                return Err("pane 不存在".into());
            }
            Ok(serde_json::json!({ "split": pane, "dir": direction }))
        }
    }
    // FakeReader 也须实现 WorkspaceWriter 才能装配成 HostState（读测试不走写端口）。
    impl WorkspaceWriter for FakeReader {
        fn set_active_workspace(&self, _id: &str) -> Result<(), String> {
            Ok(())
        }
        fn reorder_workspaces(&self, _from: usize, _to: usize) -> Result<(), String> {
            Ok(())
        }
        fn rename_workspace(&self, _id: &str, _name: &str) -> Result<(), String> {
            Ok(())
        }
        fn create_workspace(&self, _name: Option<&str>) -> Result<String, String> {
            Ok(String::new())
        }
        fn close_workspace(&self, _id: &str) -> Result<(), String> {
            Ok(())
        }
        fn save_workspace(&self, _name: Option<&str>) -> Result<String, String> {
            Ok(String::new())
        }
        fn save_workspace_to_file(
            &self,
            _id: &str,
            _name: &str,
            _path: Option<&str>,
        ) -> Result<String, String> {
            Ok(String::new())
        }
        fn delete_workspace_file(&self, _id: &str) -> Result<(), String> {
            Ok(())
        }
        fn resize_pane(
            &self,
            _ws: &str,
            _pane: &str,
            _rows: u16,
            _cols: u16,
            _alt: bool,
            _inline: bool,
        ) -> Result<(), String> {
            Ok(())
        }
        fn create_pane(&self, _pane: &str, _shell: Option<&str>) -> Result<(), String> {
            Ok(())
        }
        fn split_pane(&self, _pane: &str, _direction: &str) -> Result<Value, String> {
            Ok(Value::Null)
        }
    }

    #[test]
    fn handler_missing_workspace_is_invalid_args() {
        let accessor: Arc<dyn crate::ctx::CoreState> =
            Arc::new(HostStateAccessor(Arc::new(FakeReader(None))));
        let (ctx, _s) = ctx_with_state(accessor, CapabilitySet::allow_all());
        let err = get_workspace_snapshot(&ctx, "nope").unwrap_err();
        assert_eq!(err.kind_tag(), "invalid_args");
    }

    #[test]
    fn handler_returns_snapshot_json() {
        let raw = WorkspaceRaw {
            pane_tree: serde_json::json!({ "a": 1 }),
            pane_cwds: vec![],
            pane_titles: HashMap::new(),
        };
        let accessor: Arc<dyn crate::ctx::CoreState> =
            Arc::new(HostStateAccessor(Arc::new(FakeReader(Some(raw)))));
        let (ctx, _s) = ctx_with_state(accessor, CapabilitySet::allow_all());
        let out = get_workspace_snapshot(&ctx, "ws1").unwrap();
        assert_eq!(out["pane_tree"], serde_json::json!({ "a": 1 }));
        assert_eq!(out["git_repos"], serde_json::json!([]));
    }

    fn fake_writer(exists: bool) -> Arc<FakeWriter> {
        Arc::new(FakeWriter {
            exists,
            switched: std::sync::Mutex::new(None),
            reordered: std::sync::Mutex::new(None),
            renamed: std::sync::Mutex::new(None),
        })
    }

    #[test]
    fn switch_workspace_sets_active_via_writer() {
        let writer = fake_writer(true);
        let accessor: Arc<dyn crate::ctx::CoreState> = Arc::new(HostStateAccessor(writer.clone()));
        let (ctx, _s) = ctx_with_state(accessor, CapabilitySet::allow_all());
        switch_workspace(&ctx, "ws-42").unwrap();
        assert_eq!(*writer.switched.lock().unwrap(), Some("ws-42".to_string()));
    }

    #[test]
    fn switch_workspace_missing_is_invalid_args() {
        let writer = fake_writer(false);
        let accessor: Arc<dyn crate::ctx::CoreState> = Arc::new(HostStateAccessor(writer));
        let (ctx, _s) = ctx_with_state(accessor, CapabilitySet::allow_all());
        let err = switch_workspace(&ctx, "nope").unwrap_err();
        assert_eq!(err.kind_tag(), "invalid_args");
    }

    #[test]
    fn reorder_workspaces_forwards_indices_via_writer() {
        let writer = fake_writer(true);
        let accessor: Arc<dyn crate::ctx::CoreState> = Arc::new(HostStateAccessor(writer.clone()));
        let (ctx, _s) = ctx_with_state(accessor, CapabilitySet::allow_all());
        reorder_workspaces(&ctx, 2, 0).unwrap();
        assert_eq!(*writer.reordered.lock().unwrap(), Some((2, 0)));
    }

    #[test]
    fn reorder_workspaces_out_of_bounds_is_invalid_args() {
        let writer = fake_writer(false);
        let accessor: Arc<dyn crate::ctx::CoreState> = Arc::new(HostStateAccessor(writer));
        let (ctx, _s) = ctx_with_state(accessor, CapabilitySet::allow_all());
        let err = reorder_workspaces(&ctx, 9, 9).unwrap_err();
        assert_eq!(err.kind_tag(), "invalid_args");
    }

    #[test]
    fn rename_workspace_forwards_id_and_name_via_writer() {
        let writer = fake_writer(true);
        let accessor: Arc<dyn crate::ctx::CoreState> = Arc::new(HostStateAccessor(writer.clone()));
        let (ctx, _s) = ctx_with_state(accessor, CapabilitySet::allow_all());
        rename_workspace(&ctx, "ws-7", "新名字").unwrap();
        assert_eq!(
            *writer.renamed.lock().unwrap(),
            Some(("ws-7".to_string(), "新名字".to_string()))
        );
    }

    #[test]
    fn rename_workspace_invalid_id_is_invalid_args() {
        let writer = fake_writer(false);
        let accessor: Arc<dyn crate::ctx::CoreState> = Arc::new(HostStateAccessor(writer));
        let (ctx, _s) = ctx_with_state(accessor, CapabilitySet::allow_all());
        let err = rename_workspace(&ctx, "bad", "x").unwrap_err();
        assert_eq!(err.kind_tag(), "invalid_args");
    }

    #[test]
    fn create_workspace_returns_new_id_from_writer() {
        let writer = fake_writer(true);
        let accessor: Arc<dyn crate::ctx::CoreState> = Arc::new(HostStateAccessor(writer));
        let (ctx, _s) = ctx_with_state(accessor, CapabilitySet::allow_all());
        let out = create_workspace(&ctx, Some("foo")).unwrap();
        assert_eq!(out, Value::String("ws-created".to_string()));
    }

    #[test]
    fn close_workspace_ok_via_writer() {
        let writer = fake_writer(true);
        let accessor: Arc<dyn crate::ctx::CoreState> = Arc::new(HostStateAccessor(writer));
        let (ctx, _s) = ctx_with_state(accessor, CapabilitySet::allow_all());
        close_workspace(&ctx, "ws-3").unwrap();
    }

    #[test]
    fn close_workspace_last_or_invalid_is_invalid_args() {
        let writer = fake_writer(false);
        let accessor: Arc<dyn crate::ctx::CoreState> = Arc::new(HostStateAccessor(writer));
        let (ctx, _s) = ctx_with_state(accessor, CapabilitySet::allow_all());
        let err = close_workspace(&ctx, "ws-last").unwrap_err();
        assert_eq!(err.kind_tag(), "invalid_args");
    }

    #[test]
    fn save_workspace_returns_history_id_from_writer() {
        let writer = fake_writer(true);
        let accessor: Arc<dyn crate::ctx::CoreState> = Arc::new(HostStateAccessor(writer));
        let (ctx, _s) = ctx_with_state(accessor, CapabilitySet::allow_all());
        let out = save_workspace(&ctx, None).unwrap();
        assert_eq!(out, Value::String("hist-1".to_string()));
    }

    #[test]
    fn save_workspace_to_file_returns_path_and_rejects_empty_name() {
        let writer = fake_writer(true);
        let accessor: Arc<dyn crate::ctx::CoreState> = Arc::new(HostStateAccessor(writer.clone()));
        let (ctx, _s) = ctx_with_state(accessor, CapabilitySet::allow_all());
        let out = save_workspace_to_file(&ctx, "ws-1", "proj", None).unwrap();
        assert_eq!(out, Value::String("/saved/proj.ridge".to_string()));
        // 名空 → 底层 Err → Internal（映射自宿主消息）。
        let err = save_workspace_to_file(&ctx, "ws-1", "  ", None).unwrap_err();
        assert_eq!(err.kind_tag(), "internal");
    }

    #[test]
    fn delete_workspace_file_ok_via_writer() {
        let writer = fake_writer(true);
        let accessor: Arc<dyn crate::ctx::CoreState> = Arc::new(HostStateAccessor(writer));
        let (ctx, _s) = ctx_with_state(accessor, CapabilitySet::allow_all());
        delete_workspace_file(&ctx, "ws-1").unwrap();
    }

    #[test]
    fn get_pane_layout_for_forwards_workspace_id_to_reader() {
        let writer = fake_writer(true); // FakeWriter.pane_layout 回带 id 的值
        let accessor: Arc<dyn crate::ctx::CoreState> = Arc::new(HostStateAccessor(writer));
        let (ctx, _s) = ctx_with_state(accessor, CapabilitySet::allow_all());
        let out = get_pane_layout_for(&ctx, "ws-77").unwrap();
        assert_eq!(out, serde_json::json!({ "for": "ws-77" }));
        // active 版走 active_workspace()（FakeWriter 为空串）。
        let out2 = get_pane_layout(&ctx).unwrap();
        assert_eq!(out2, serde_json::json!({ "for": "" }));
    }

    #[test]
    fn get_pane_scrollback_handlers_forward_args_to_reader() {
        let writer = fake_writer(true);
        let accessor: Arc<dyn crate::ctx::CoreState> = Arc::new(HostStateAccessor(writer));
        let (ctx, _s) = ctx_with_state(accessor, CapabilitySet::allow_all());
        let tail = get_pane_scrollback_tail(&ctx, "p-1", 4096).unwrap();
        assert_eq!(tail, serde_json::json!({ "tail": "p-1", "max": 4096 }));
        let before = get_pane_scrollback_before(&ctx, "p-1", 99, 2048).unwrap();
        assert_eq!(
            before,
            serde_json::json!({ "pane": "p-1", "before": 99, "max": 2048 })
        );
    }

    #[test]
    fn list_native_sessions_returns_reader_value() {
        let writer = fake_writer(true);
        let accessor: Arc<dyn crate::ctx::CoreState> = Arc::new(HostStateAccessor(writer));
        let (ctx, _s) = ctx_with_state(accessor, CapabilitySet::allow_all());
        let out = list_native_sessions(&ctx).unwrap();
        assert_eq!(out, serde_json::json!([{ "name": "bg-sess" }]));
    }

    #[test]
    fn resize_pane_ok_and_pane_missing_is_invalid_args() {
        let writer = fake_writer(true);
        let accessor: Arc<dyn crate::ctx::CoreState> = Arc::new(HostStateAccessor(writer.clone()));
        let (ctx, _s) = ctx_with_state(accessor, CapabilitySet::allow_all());
        resize_pane(&ctx, "ws-1", "p-1", 40, 120, false, false).unwrap();
        // pane 不存在（FakeWriter.exists=false）→ InvalidArgs。
        let w2 = fake_writer(false);
        let acc2: Arc<dyn crate::ctx::CoreState> = Arc::new(HostStateAccessor(w2));
        let (ctx2, _s2) = ctx_with_state(acc2, CapabilitySet::allow_all());
        let err = resize_pane(&ctx2, "ws-1", "nope", 40, 120, false, false).unwrap_err();
        assert_eq!(err.kind_tag(), "invalid_args");
    }

    #[test]
    fn create_pane_ok_and_spawn_failure_is_internal() {
        let writer = fake_writer(true);
        let accessor: Arc<dyn crate::ctx::CoreState> = Arc::new(HostStateAccessor(writer));
        let (ctx, _s) = ctx_with_state(accessor, CapabilitySet::allow_all());
        create_pane(&ctx, "p-1", Some("bash")).unwrap();
        // 起进程失败（FakeWriter.exists=false）→ Internal。
        let w2 = fake_writer(false);
        let acc2: Arc<dyn crate::ctx::CoreState> = Arc::new(HostStateAccessor(w2));
        let (ctx2, _s2) = ctx_with_state(acc2, CapabilitySet::allow_all());
        let err = create_pane(&ctx2, "p-1", None).unwrap_err();
        assert_eq!(err.kind_tag(), "internal");
    }

    #[test]
    fn split_pane_returns_result_and_failure_is_internal() {
        let writer = fake_writer(true);
        let accessor: Arc<dyn crate::ctx::CoreState> = Arc::new(HostStateAccessor(writer));
        let (ctx, _s) = ctx_with_state(accessor, CapabilitySet::allow_all());
        let out = split_pane(&ctx, "p-1", "horizontal").unwrap();
        assert_eq!(
            out,
            serde_json::json!({ "split": "p-1", "dir": "horizontal" })
        );
        let w2 = fake_writer(false);
        let acc2: Arc<dyn crate::ctx::CoreState> = Arc::new(HostStateAccessor(w2));
        let (ctx2, _s2) = ctx_with_state(acc2, CapabilitySet::allow_all());
        let err = split_pane(&ctx2, "p-1", "vertical").unwrap_err();
        assert_eq!(err.kind_tag(), "internal");
    }
}
