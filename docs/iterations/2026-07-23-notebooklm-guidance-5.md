# NotebookLM guidance — iteration 5 收口 → iteration 6 规划

生成时间：2026-07-23 15:40 +08:00 · 来源：唯一 `PROJECT-STATE`（source `1e359c0c`）
Conversation：`a47d3199-c1f9-47f1-927c-ff2c4875b77d`

## NotebookLM 原始指导（摘要）

1. Q1：**同意 P1 Remote Agent 控制台 MVP 为 iteration 6 主线**；宣告新 capability `agent_roster_view`，方法 `get_teammate_topology()` + `subscribe_agent_updates()`；称 rdg-Host 也 Supported（JSON 输出）。
2. Q2：投影脱敏方案——core 侧把 Teammate 转 `AgentSnapshot`，严禁映射 `mcp_endpoint/token/env_vars`。
3. Q3：S1 遥测（F1/F2）并入本轮作准入护栏，落 `telemetry.log` JSON 条目。
4. Q4：`-p ridge --lib` loader 错误挂起为低优先环境债务。Q5：并行排 A1 只读同源化 + T3 CI 挂钩。
5. 目标清单 6 项 + 减法「从构建脚本移除 WebGPU 实验检查」。

## 对抗评审（checker：codegraph + 源码 + 仓库事实）

| 候选 | 裁决 | 关键证据 |
| --- | --- | --- |
| P1 为主线 | **采纳** | 与 GAP §7 近期组合顺位一致；可信基线包已闭。 |
| capability 名 `agent_roster_view` | **改名 `teammate`** | 既有能力名皆短名词（pane/fs/git/search/workspace/theme，`capabilityContract.ts`）；命名一致性。 |
| `subscribe_agent_updates` 订阅流 | **驳回（MVP）** | Remote RPC 面现状为 request/response + pane 专用流；为 roster 新建流类型属协议面扩张，违反最小 MVP。轮询 `get_teammate_topology` 即可，间隔由 UI 定。 |
| rdg-Host Supported | **驳回，改 denied** | teammate server 侧表在桌面 `AppState`（`commands/teammate.rs` 映射 `Workspace` 侧表）；rdg 无头 host 无 roster 数据源，且 `CLI_CAPABILITIES=[pane,fs,search]`。矩阵列 denied。NotebookLM 另造「CLI-Local」列，非矩阵六入口，弃。 |
| 投影脱敏 | **采纳（现状即净）** | `topology_json` 载荷仅 id/name/paneId/paneIndex/role/status/capability + leaderId/edges（`teammate.rs:69-84`），本无 endpoint/token；验收补「序列化拓扑不得含 token/endpoint/env 字段」断言，覆盖 typed-profiles 路径（`profiles::topology_for`）。 |
| S1 遥测落 `telemetry.log` 文件 | **驳回措辞，按设计文档实施** | 设计（2026-07-23-s1-…-design.md）定的是「现有 tracing/log 计数 + SPA 本地诊断计数，第一阶段人工读数」；新增日志文件是新持久面。实施 F1（transcript_present）/F2（binding mode）计数 + 单测断言恰好计数一次。 |
| A1 只读三件套同源化 | **采纳（带停机条件）** | `commands/workspace.rs:22` 与 core `commands/workspace.rs:161` 双实现实锤（iteration 5 审计）。 |
| T3 挂 CI | **驳回（本轮）** | 仓库 workflows 仅 deploy-pages/release，无测试 CI；prod 探测需 GH secrets 配置，属用户决策。保留为 runbook 命令。 |
| 「Pane 切换高亮」独立目标 | **并入主线验收** | 属 MVP 交互细节，不独立成目标。 |
| 减法：移除 WebGPU 构建检查 | **驳回** | 违反 E1 自身规则「先证伪再删」；无真机测量前不动构建面。 |

## 经检验后的 iteration 6 方向

见 `CONTRACT-iteration-6.md`：主线 P1（capability `teammate` + 只读 `get_teammate_topology` 跨入口暴露 + Remote roster 视图 + 脱敏断言），支线 S1 遥测计数、A1 workspace 只读三件套同源化；A2 矩阵随新能力同步更新（G1 验收内）。
