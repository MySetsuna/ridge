# A1 共享内核减法审计（iteration 5 / G5）

日期：2026-07-23 · 方法：rustc dead_code + grep 零调用者核对 + codegraph 调用面抽样 + 委托密度采样。
原则（GAP_PORTFOLIO A1）：每次只删除**一个**有等价共享路径且有回归覆盖的重复面；不为凑数删码。

## 1. 本轮示范删除（已执行）

**`src-tauri/src/state.rs` pane-output 通道死面**（`PaneOutputSender` 类型 + `PaneRegistry.output_cb` 字段 +
`register/unregister/get_pane_output_channel` 三方法 + `is_empty` 关联支 + 文档行）：

- 等价共享路径：desktop 走 `delta_cb`（GridDelta 增量帧），Remote 走 `remote_subs` raw fan-out
  （`subscribe_pane_raw` → `RemotePtyEvent`）；旧「coalesced pty-output」通道无任何生产写读。
- 证据：rustc dead_code（getter 无调用 ⇒ 存入的回调永不被取出/调用）；grep 全仓 `output_cb|PaneOutputSender`
  仅命中 state.rs 自身；删除后 `cargo check -p ridge` 通过、`cargo test -p ridge --bins` 27/27、
  对应 dead_code 告警消失。净 diff −45 行。

## 2. 双路径现状图（workspace / pane / git 面）

| 命令面 | 委托密度（`ridge_core::` 引用） | 判定 |
| --- | --- | --- |
| `commands/git.rs` | 56 处 / 49 fn | **已迁移**：薄委托样板（A1 目标形态） |
| `commands/pane.rs` | 1 处 | 多为桌面特有（Tauri 事件/窗口生命周期），待逐项分类 |
| `commands/workspace.rs` | 0 处 | **真双路径**：`list_workspaces`（:22）等与
  `ridge-core/src/commands/workspace.rs`（:161 起 dispatch handler）各自实现同一领域结果 |

下一 A1 切片（候选，非本轮）：`commands/workspace.rs` 只读三件套
（`list_workspaces` / `get_active_workspace_id` / `get_workspace_snapshot` 同源化）改为经 core handler + 端口注入，
一次一件、每件配跨入口回归（capabilityContract/conformance 已有断言可扩）。写路径（create/close/switch）
涉及 PTY 生命周期与桌面事件桥，风险高，排最后。

## 3. 其余编译器已证死面（本轮不删，逐项留判）

| 位置 | 项 | 备注 |
| --- | --- | --- |
| `packages/ridge-core/src/workspace/pane_tree.rs:711,719` | `first_leaf` / `last_leaf` | core 公共 API，可能为未接线的导航预留；查 CLI/桌面规划后定 |
| `src-tauri/src/engine/parser.rs:215` | `full_reframe_with_scrollback` | 与 resync 路径重叠疑似被 RIS+scrollback 方案取代；确认后删 |
| `src-tauri/src/hosts/mod.rs:30-33` | `HostStatus::{Connecting,Connected,Error}` | H1（远端 host live PTY）未闭环的预留态；H1 决策前保留 |
| `packages/ridge-cli/src/device_flow.rs` | 常量/结构/`run_enable` 等 | 设备码登录流疑似被 `login_flow` 取代；确认后整文件评估 |
| `packages/ridge-cli/src/login_flow.rs:44` | `UserBrief.is_trial` | 反序列化字段，删需对齐服务端 payload |
| `packages/ridge-cli/src/tui/workspace.rs:178+` | 3 个未用方法 | TUI 演进残留 |

## 4. 结论

- A1 不是「继续搬迁」而是「消真双路径」：git 面已达标；workspace 只读面是下一个最小可验证切片。
- 死面删除以编译器为 checker 最安全，但仍须逐项排除「为未来预留」的 API（§3 各项留判即此）。
