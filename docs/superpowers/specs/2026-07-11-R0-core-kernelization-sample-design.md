# R0 样板：ridge-core 内核化——git 只读 + workspace/pane 快照收口

> 日期：2026-07-11 · 状态：待实现（路线图 R0 的第一个样板 spec）
> 上游：`2026-07-11-remote-rdg-product-engineering-roadmap.md`（主线 C）· `docs/plans/s1-migration-ledger.md`（既有迁移账本）
> 任务：升级 backlog #19 的第一刀

## 0. 关键发现（校正认知）

内核化**范式早已生产级落地**，R0 是**沿账本续迁**而非新建架构：
- `packages/ridge-core/src/dispatch.rs`：`dispatch(method:&str, args:Value, ctx:&Ctx) -> Result<Value, CoreError>` 单一 seam，四道门（能力准入 D8 → 路径穿越 → 沙箱 root-scope → 方法表），未迁方法落 `MethodNotFound`（迁移窗口内宿主桥回退 legacy，零回归）。
- `packages/ridge-core/src/ctx.rs`：`Ctx` = `CoreState`(下沉宿主状态，`as_any` 下转) + `EventSink`(D11 Broadcast/Connection 分流) + `TaskSpawner`(tokio-free) + `CapabilitySet` + `connection_id`。**这就是路线图 C6 的端口-适配器**。
- `commands::{settings,shell,theme}` + `fs::commands`：fs 读写/search/shell/settings/theme 已按此迁移，**纯 handler 取 `ctx` + args**；`settings::HostStateAccessor` 是「宿主状态经 trait 端口注入」的现成模板。
- `ctx::test_support`：`EmptyState`/`RecordingSink`/`NoopSpawner` 让纯逻辑单测**零 Tauri、零 cdylib**（本机 app crate `cargo test` 会崩，见账本）——**这就是路线图 C5 的可测地基**。
- `packages/ridge-core/src/commands/git.rs`：**git 只读逻辑已在 ridge-core**（`GitRepoInfo`/`GitDiffStatus`/`git_diff_summary`/`git_op_in_progress`/`git_get_file_versions`/`get_git_branches`…纯函数，取 `repo_root:String`/`&Path`）。**缺的只是 dispatch 路由**，不是重写。

结论：R0 = ①把已在 core 的 git 只读**接进 dispatch 方法表 + 能力集**；②把 `snapshot_workspace`（现耦合桌面 `AppState`，在 `src-tauri/src/commands/ridge_file.rs`）经 **`WorkspaceReader` trait 端口**下沉 core。两者都照 fs/settings 现成范式，是范式的**复制**不是发明。

## 1. 目标与验收（把 C5/C6 钉进样板）

产出一个可复用模板：以后每个领域收口都照抄。样板交齐 5 件（路线图 §5）：
1. `ridge-core` 纯 handler（不 `use tauri`/`axum`，经 `Ctx`/trait 端口取数据）；
2. `CoreError` 分类 + 零 panic（不 `unwrap`/`expect`/越界）；
3. trait 端口（git 无需——纯函数取 `repo_root`；workspace 需 `WorkspaceReader`）+ 桌面 adapter + rdg adapter；
4. `ctx::test_support` 驱动的 host-side 单测（钉输入校验/错误分支/不变式）；
5. 桌面壳与 rdg 无头壳跑通同一 method、结果一致（rdg 不再 `MethodNotFound`）。

## 2. 子样板 A：git 只读接入 dispatch（低风险先行）

### 范围（只读、无副作用、rdg 最急需）
路由这批已存在的 core 纯函数（方法名对齐桌面 legacy dispatcher 与 rdg 控制协议）：
- `get_git_info`（→ `commands::git` 取 `GitRepoInfo`：graph+branches+status）
- `git_diff_summary`（pane git pill 数据源）
- `git_op_in_progress`（cherry-pick/revert/merge/rebase 状态）
- `git_get_file_versions`（diff 查看器 original/modified）
- `git_list_branches`

### 改动
- `dispatch.rs` 方法表新增分支（照 §187-315 fs 范式）：解析 args 的 `repoRoot`/`root` → 调 `commands::git::*` → `serde_json::to_value(..).map_err(CoreError::internal)`。异步的（`git_diff_summary` 是 `async`）在 handler 内 `.await`（dispatch 已是 async 上下文——核对签名，如需则包 `futures` 或改 spawn_blocking，按现有 async 命令范式）。
- `capability.rs`：把这批 method 加入**远程可读**能力集（与 fs 只读同档；确认它们进 `remote` set 而非 host-only）。
- `PATH_KEYS` 已含 `repoRoot`/`root`/`cwd` → 穿越/沙箱守卫自动覆盖，无需改。

### 测试（`dispatch.rs` #[cfg(test)]，照 §321 现有范式）
- `dispatch("git_op_in_progress", {repoRoot: <临时 git dir>}, ctx)` 返回结构正确。
- `dispatch("git_diff_summary", {repoRoot: <非 git dir>}, ctx)` 优雅返回 added/removed=0（git.rs 契约：非仓库不 error）。
- 能力集不含时 → `CapabilityDenied`；`repoRoot` 带 `..` → `PathTraversal`。

## 3. 子样板 B：workspace/pane 快照经 `WorkspaceReader` 端口下沉

### 端口（新增 `ridge-core/src/commands/workspace.rs`）
```rust
/// 宿主暴露「读某工作区的 pane 树快照」的端口（桌面=AppState，rdg=daemon 状态）。
pub trait WorkspaceReader: Send + Sync {
    /// 返回该工作区 pane_tree 的 JSON 快照 + 其中各 pane cwd 命中的 git 仓库根集合。
    /// 找不到工作区 → None（handler 映射 CoreError::InvalidArgs）。
    fn workspace_snapshot(&self, workspace_id: &str) -> Option<WorkspaceSnapshot>;
}
pub struct WorkspaceSnapshot { pub pane_tree: serde_json::Value, pub git_repos: Vec<String>, pub pane_titles: std::collections::HashMap<String,String> }
```
- handler `get_workspace_snapshot(ctx, workspace_id)`：`ctx.state::<H>()`... 不行（跨两种宿主）→ 照 `settings::HostStateAccessor` 范式：`WorkspaceReader` 由**宿主状态实现**，handler 经一个 `ctx` 上的访问点取 `&dyn WorkspaceReader`。**落地细节**：确认 `settings::HostStateAccessor` 当前如何从 `Ctx` 取到 trait 对象（是 downcast 到实现了该 trait 的具体 state，还是 Ctx 持有 trait 对象）——照抄同款接法，保持一致。
- `snapshot_workspace` 的纯组装逻辑（tree_json + git_repos BTreeSet 去重 + pane_titles）从 `ridge_file.rs` 搬进 core handler；`find_git_root` 也搬（或复用 core 已有等价物）。

### adapters
- **桌面**：`src-tauri` 为 `AppState`(或其包装) 实现 `WorkspaceReader`——读 `workspaces.get(id).pane_tree` + `teammate_pane_titles`，即现 `snapshot_workspace` 前半段。`save_workspace_to_file` 改为「调 core 拿 snapshot → 本地 atomic_write」（写盘留桌面壳，快照组装归 core）。
- **rdg**：为 daemon 的 workspace 状态实现同 trait（`packages/ridge-cli` 的 `Workspace`）。

### 测试
- core：`FakeWorkspaceReader` 返回固定快照 → `get_workspace_snapshot` 组装/去重/None→InvalidArgs 全测（`ctx::test_support` 驱动，零 Tauri）。
- 两端 adapter 各一个「同一 workspace_id 返回等价 pane_tree」的用例（桌面可 host 单测，rdg 同）。

## 4. 顺序与门禁（每步 `cargo test -p ridge-core` 绿 + 不碰 legacy 回退）
1. 子样板 A：git 只读路由 + 能力集 + dispatch 测试 → commit。
2. 子样板 B-1：`WorkspaceReader` 端口 + core handler + core 单测 → commit。
3. 子样板 B-2：桌面 adapter + `save_workspace_to_file` 改接 core → `cargo check -p ridge` → commit。
4. 子样板 B-3：rdg adapter + rdg 无头壳路由该 method → commit。
5. 账本 `s1-migration-ledger.md` 记一笔（git-read + workspace-snapshot 已迁）+ 本 spec 标完成。

## 5. 不做（YAGNI）
- 不动 pane/terminal 的**写**命令（split/create/resize——有副作用、涉及 PTY 生命周期，留后续 R0 迭代，样板先立只读读快照的范式）。
- 不重构 dispatch 四道门/Ctx/CoreError（已生产级，照用）。
- 不追一次性全量迁移——样板立范式，其余命令滚动续账本。

## 6. 风险
- git `async` 命令在 dispatch（同步 or 异步？）里的接法需核对 dispatch 签名——若 dispatch 非 async 则这批走宿主既有 async 包装，或本刀只迁**同步** git 只读（`git_op_in_progress` 是同步），把 async 的 `git_diff_summary` 留下一刀。**先核对 dispatch 是否 async 再定 A 的确切子集**。
- `WorkspaceReader` 从 `Ctx` 取 trait 对象的确切接法必须与 `settings::HostStateAccessor` 完全一致，避免引入第二种范式（违背 C6「不要第二份副本」）。
