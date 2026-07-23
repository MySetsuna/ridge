# Workspace 写路径审计（A1，iteration 9 G5——零代码变更）

日期：2026-07-23。范围：工作区增删改（create/close/rename/reorder/switch/save）在 **Tauri 命令层**（`src-tauri/src/commands/workspace.rs`）与 **core 写端口**（`remote_bridge.rs::impl WorkspaceWriter for AppState`，供 `ridge_core::commands::workspace` dispatch 远端路径）的双落点核查。方法：逐函数对读两侧实现。

## 结论总表

| 操作 | 桌面命令 | 写端口（远端路径） | 判定 |
| --- | --- | --- | --- |
| create | `create_workspace` → `create_workspace_core` | `writer.create_workspace` → **同一** `create_workspace_core` | ✅ 已单源（DRY；`insert_new_workspace` 为 `Workspace` 字面量唯一产地） |
| save / save_to_file | → `save_workspace_core` / `save_workspace_to_file_core` | → **同一** core fn | ✅ 已单源 |
| switch / set_active | 校验存在 + 置 `active_workspace`（8 行，无广播） | 逐字同义副本（无广播） | ⚠ 双副本（小，行为一致） |
| reorder | 越界拒 + remove/insert（8 行，无广播） | **逐字副本**（注释自认「逐字一致」） | ⚠ 双副本（小，行为一致） |
| rename | 改名 + `schedule_auto_save` + WorkspaceRenamed/WorkspacesChanged/WorkspaceListChanged 三广播（~25 行） | **逐字副本**（注释自认） | ⚠⚠ 双副本（肥，含广播链） |
| close | 最后一个拒 + 顺序表/映射移除 + 活动区改选 + 双广播（~35 行） | **逐字副本**（注释自认「语义一致」） | ⚠⚠ 双副本（最肥，含锁序 + 广播链） |
| close（LAN 路由） | — | `remote_host_impl.rs::close_workspace`（HostServer trait） | ⚠⚠⚠ **第三副本且已分歧**（见下） |

**已发现行为分歧（唯一）**：LAN host 第三副本的 close ①**漏发** `WorkspacesChanged`/`WorkspaceListChanged` 双广播——LAN 端关工作区后，其他 remote 客户端与桌面前端**不会**收到列表变更通知；②**多删** `workspace_names` 条目（另两副本不删，靠 Workspace drop 留名残条）；③移除顺序不同（先 map 后 order）。其余操作（switch/reorder/rename/close 桌面 vs 写端口）逐字核对一致；风险在演化：任一侧单独改动即静默分叉。

## 同源化候选顺序（未来实现轮的合同素材，本轮不动）

1. **close（升级为缺陷修复级）**：抽 `close_workspace_core(&AppState, id) -> Result<(), String>`，**三侧**委托（模式照抄 `create_workspace_core`，净删 ~60 行），顺带修 LAN 副本漏广播的实际缺陷；`workspace_names` 清理语义三方对齐后统一。锁序不变，风险低-中。
2. **rename**：同法抽 `rename_workspace_core`（净删 ~25 行）。
3. switch/reorder：各 8 行，收益小，可顺手同批。
4. 守卫建议：同源化后加一条「写端口与桌面命令共用 core fn」的结构测试（grep 断言写端口不直接操作 `workspace_order`/`workspaces` 字段），防回潮。

## 边界（明确不入候选）

- `restore_workspace` 刻意不下沉 dispatch（远端还原属安全面扩张，需显式授权决策——core 侧注释已锁定）。
- `remote_host_impl.rs` 的其余 HostServer 方法多为薄委托适配层；唯 `close_workspace` 例外（见上第三副本）。
