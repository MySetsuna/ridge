# rdg 无头 host 语义缺口报告（C1，自动派生）

生成：`node scripts/rdg-gap-report.mjs`（源 = `docs/capability-matrix.json`，由 2 条一致性测试守卫）。手改无效，重跑脚本刷新。

## rdg 已支持能力（3）

- **pane**：3 方法（get_pane_layout, write_to_pty, resize_pane）
- **fs**：2 方法（get_file_tree, read_file）
- **search**：1 方法（text_search）

## rdg 缺口（denied，5）——桌面/云 host 有而 rdg 无

| 能力 | 方法数 | 缺失语义 | 收口判定 |
| --- | --- | --- | --- |
| invoke | 0 |  | 不宣告即语义完备（控制器最小方法集为空，无可路由项） |
| git | 3 | get_scm_status, get_git_info_with_cwd, git_diff_file | 补路由候选（ridge-core dispatch 已共享实现，待真实 rdg 场景需求触发接线） |
| workspace | 3 | list_workspaces, get_active_workspace_id, get_workspace_snapshot | 补路由候选（同上；接线时须过 A2 宣告纪律与合同测试） |
| theme | 1 | get_theme_data | 永久缺口（rdg 无 UI 宿主，无主题渲染面；不宣告即语义完备） |
| teammate | 2 | get_teammate_topology, list_hitl_pending | 刻意排除（无头环境无 Agent Center 宿主；重开需 D6 安全评审） |



## 语义一致性原则（锁定决策）

能力必须先协商宣告，未宣告入口**显式拒绝**而非静默分叉（跨入口合同测试守卫）。rdg 对 denied 能力的正确行为 = 不宣告 + 拒绝对应方法调用；上表「收口判定」列指导后续是否补齐路由。
