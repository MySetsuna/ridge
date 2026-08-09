# 下一迭代交接：NLM 愿景盘点、临时来源与 Sonar 覆盖率

日期：2026-08-09  
状态：盘点与需求登记完成；并行 worker 未形成可验收结果，已由主 Agent 完成本地归纳。不得把 Ridge 投递回执写作 worker 完成。

本轮后续深化已新增 `REQ-AGENT-COMMUNICATION-ARCH-REBUILD-01`；详尽正文见 `docs/iterations/2026-08-09-agent-communication-architecture-requirement.md`，来源为 `9516749e-c317-4f13-9cda-b64b00cec465`。

## 已登记目标

- `REQ-NLM-ITERATION-01`：NLM 仅提供候选；须经本地代码、CodeGraph、测试与运行证据后落地。
- `REQ-NLM-CLOSURE-20260809`：闭合现场验收后，继续盘点来源、对话、note 并形成下一批候选。
- `REQ-SONAR-COVERAGE-80-01`：下一迭代把 **Sonar 全项目测试覆盖率提升至 ≥80.0%**；必须有真实 Sonar scan、项目级 Quality Gate/API 与 coverage 证据，禁止以 restricted scan、`new_coverage` 或本地估算替代。

对应需求已写入 `docs/REQUIREMENTS-SPEC.md`，`requirements_gate.py assert-task-executable` 结果为 `executable: no pending requirements`。

## 近七日 NLM 全量盘点

按 notebook 最近修改时间筛出 5 本活跃笔记，共 36 个来源、14 个对话、6 个 note：

| Notebook | ID | 来源/对话/note | 处理结论 |
|---|---|---:|---|
| Ridge 项目现状、愿景与规划基线 | `66919cb9-1329-4ddf-955c-f426d15a9fe6` | 3/1/2 | Agent 通信、kernel-first；主体已有 Active REQ，保留 residual |
| RidgeCode现状、愿景、路线图 | `24ef1fd0-37f5-4aa8-9a6d-53fe577b6b57` | 27/1/2 | TUI scrollback/Lean Output 等候选；本仓缺少对应核心符号，疑似跨仓 |
| nlm-mcp live deep research 20260804-234655 | `f6ffd900-708d-44ee-9818-1a3269c533fc` | 2/1/0 | **临时对话来源重点**，安全与导入幂等约束纳入审计 |
| 基于 NotebookLM 与 CodeGraph 的迭代开发工作流 | `2bf9b409-7b68-4e9c-8bb4-66036003e2c3` | 2/1/1 | CodeGraph-first、token 计量已有流程约束 |
| Agent 视频工作室｜产品与技术路线调研 | `b5d5bff3-4167-4d79-aba1-678a89f3177b` | 2/2/1 | 与 wind 当前边界无对应实现，out-of-scope |

## 下一迭代需求清单

### P0：Sonar 覆盖率 ≥80%

1. 固化全项目 coverage baseline，明确语言/模块/排除项；排除项须有理由，不得借排除制造达标。
2. 按未覆盖代码分波次补确定性测试，优先高风险共享边界、生命周期、错误路径。
3. 运行完整测试与真实 Sonar scan，保存输入命令、scan task、Quality Gate/API、coverage 与失败原因。
4. 只有项目级 `coverage >= 80.0` 且 Quality Gate OK 才可关闭；环境阻塞则记录差距，不得宣称完成。

当前证据仅证明 Sonar 新问题/受限文件门禁等局部结果，不能证明全项目 80%。

### P1：Agent communication/message hub residual

来源：`9516749e-c317-4f13-9cda-b64b00cec465`，对话 `a47d3199-c1f9-47f1-927c-ff2c4875b77d`。候选内容：typed message/task/event/control/artifact、inbox/topic/ack/idempotency、headless rdg/MCP adapter。

在途覆盖：`REQ-RIDGE-MCP-AS-KERNEL-API-01`、`REQ-AGENT-COMMUNICATION-REGISTRY-01` 已覆盖内核/API/registry。下一迭代只处理经 CodeGraph 证明的 residual，禁止新增第二身份源或重复 Commune UI。

### P1：临时 ChatGPT Web 来源的 bridge 安全审计

来源：Notebook `f6ffd900-708d-44ee-9818-1a3269c533fc` 的临时 source `df4d5dcc-9813-4c61-ae9f-1e9199cb7555`，标题 `chatgpt-web-1785898376338-84776-0.md`。

必须保留的约束：不用 ChatGPT 浏览器 cookie 充当 API 凭据；浏览器自动化仅用专用 profile；优先官方 API；个人 NotebookLM 仅受控 fallback；导入校验 MIME/大小/SHA256/幂等；轮询有界；不记录 token/cookie；检查 localhost、Origin/Host、token audience、CSRF、DNS rebinding、MFA/CAPTCHA 边界。

本仓当前未证明存在对应产品 bridge。故下一步是审计并补证，不以流程文档伪造代码实现；不得向 NLM 写入、删除或上传 source/note。

### P2：RidgeCode TUI 候选（先判跨仓）

RidgeCode 27 个来源、对话 `68791fb7-659a-4ad6-a86c-beb7ac694781` 与 note 提出原生物理 scrollback、Lean Output、LifecycleAnchor、DECSTBM、点击区域、KKP、grapheme/断点布局等。

本仓 CodeGraph 未找到 `LifecycleAnchor`、`insert_before`、`DECSTBM`、`ClickRegionRegistry` 等核心符号；因此暂不改 wind。确认属于另一仓后转外部 backlog；若确认属于本仓，再以具体路径、测试与 Active REQ 重新 intake。

## 已排除项

- `REQ-SCM-GIT-SCAN-DEPTH-01` 与 pane branch-pill 规则已有 Active REQ，且 CodeGraph 显示硬上限 1 与直接子目录测试，不算“无在途”。
- Agent 视频工作室研究无 wind 本仓路径/Active REQ，out-of-scope。

## Ridge 派发审计

派发包：`.iteration/agents/nlm-next-iteration-synthesis.json`  
计划：`.iteration/agents/dispatch-plan.json`  
派发记录：`.iteration/ridge-dispatch-evidence-20260809.json`

- pane 已创建：`332c4045-cf5a-4840-83ef-d3acf9443f53`。
- `terminalAccepted=true`；`agentAcknowledged=false`。
- Codex 首次启动先完成更新，随后 `codex_apps` MCP transport 反复超时；隔离配置仍受同一宿主 transport 阻断。
- 不再向 Codex 以外 CLI 派发；Claude OAuth 已过期，Grok 尝试已按用户要求停止。
- 未产生 `result-nlm-next-iteration-synthesis.json`，故未运行通过 `validate-result`，不宣称并行归纳完成。

## 交接入口

- 候选明细：`docs/iterations/2026-08-09-nlm-next-iteration-candidates.md`
- 本轮 NLM 现场记录：`docs/iterations/2026-08-09-nlm-iteration-final.md`
- Sonar 现场记录：`docs/iterations/2026-08-09-nlm-sonar-e2e-evidence.md`
- 需求总表：`docs/REQUIREMENTS-SPEC.md`
- Ridge 能力原始快照：`.iteration/ridge-launch-capabilities-20260809.txt`

## 本轮已落地：Agent 通信契约首波

- 代码：`packages/ridge-core/src/teammate/communication.rs`；由 `ridge_core::teammate` 与 `ridge_core` 公共面导出。
- 已实现：`AgentIdentity`（session/workspace/pane/CWD/executable/argv/generation/lease/lifecycle/online/last_seen/capabilities）、`AgentEnvelope`（message/task/event/control/artifact/reply、idempotency、conversation/task、from/to、sequence、priority、deadline/cancel、payload/artifact、ack/nack）、generation/lease/workspace/capability 目标校验、稳定 typed errors。
- 已实现：固定 Delivery Engine 优先级 `RuntimeApi → A2a → McpPull → PtyFallback`；PTY 只有五条件全真才可选，可靠性明确标为 `BestEffort`。
- 验证：`cargo test -p ridge-core teammate::communication -- --nocapture`，5 passed / 0 failed；完整 `cargo test -p ridge-core --lib` 在本轮代码变更前为 322 passed / 0 failed，PTY 收紧后已再跑目标测试。
- 结论：该波只建立 Kernel domain seam，不声称 Hub/MCP/Remote 适配器已完成；下一波接 `ridge-kernel` registry/lifecycle 与 MCP roster/message 工具，随后接 desktop/Remote 投影。
- 本轮追加：`TopologyGraph` 内嵌同一份 `AgentIdentity` map，提供 `commit_online_agent`、`validate_agent_target`、`agent_identities`；成功提交要求身份字段完整且 online/lifecycle 可接收，旧 generation/lease 与失败尝试不覆盖 active，`remove_teammate` 同步清理 identity。
- 新增拓扑验收：幂等提交/销毁清理、offline/旧 generation 拒绝、同一 Kernel roster 快照 target 校验；目标测试 `15 passed / 0 failed`。
- MCP 首波：`ridge-mcp` 新增并路由 `ridge_send_message`、`ridge_create_task`、`ridge_publish_event`、`ridge_fetch_inbox`、`ridge_task_update`、`ridge_list_agents`；旧工具保留。新消息进入有界 inbox/receipt，返回 `messageId/taskId/deliveryId`、`queued`、`mcp_pull`、`at_least_once`，不把 `terminalAccepted` 冒充 Agent 已处理。
- MCP 验证：`cargo test -p ridge-mcp --lib`，71 passed / 0 failed；此为 Phase-1 内存 Hub 兼容层，SQLite 持久化与 Kernel generation/lease 适配仍在途。
