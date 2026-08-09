# NLM 近七日下一迭代候选盘点

日期：2026-08-09。范围：NLM 最近七日有活动的笔记，以及其全部来源、对话、note；NLM 只作候选假设，落地须以本地代码、CodeGraph、测试与运行证据为准。

## 活跃笔记与来源

| 笔记 | notebook id | 来源 | 对话 | note | 结论 |
|---|---|---:|---:|---:|---|
| Ridge 项目现状、愿景与规划基线（2026-07-21） | `66919cb9-1329-4ddf-955c-f426d15a9fe6` | 3 | 1 | 2 | Agent 通信、kernel-first、远端/物理证据候选；多项已有 Active REQ |
| RidgeCode现状、愿景、路线图 | `24ef1fd0-37f5-4aa8-9a6d-53fe577b6b57` | 27 | 1 | 2 | TUI 原生 scrollback、Lean Output、物理闭合等候选；需先确认是否属于本仓 |
| nlm-mcp live deep research 20260804-234655 | `f6ffd900-708d-44ee-9818-1a3269c533fc` | 2 | 1（空对话） | 0 | **临时对话来源重点**，含 ChatGPT Web 深研报告；安全/导入约束需审计本地闭环 |
| 基于NotebookLM与codegraph的迭代开发工作流 | `2bf9b409-7b68-4e9c-8bb4-66036003e2c3` | 2 | 1 | 1 | CodeGraph-first、token 流程已有工作流要求 |
| Agent 视频工作室｜产品与技术路线调研 | `b5d5bff3-4167-4d79-aba1-678a89f3177b` | 2 | 1 | 1 | 独立产品方向，不纳入 wind 本迭代 |

临时对话来源：`df4d5dcc-9813-4c61-ae9f-1e9199cb7555`，标题 `chatgpt-web-1785898376338-84776-0.md`。其核心约束为：优先官方 OpenAI Responses API；不得把 ChatGPT 浏览器 cookie 当 API 凭据；浏览器自动化须用专用 profile；NotebookLM Enterprise 优先 API、个人版 UI/Drive 仅作受控 fallback；导入要有 MIME/大小/SHA256/幂等；轮询有界；不记录 token/cookie；localhost、Origin/Host、token audience、CSRF/DNS rebinding、MFA/CAPTCHA 均须设边界。该来源已进入本次派发包，不能遗漏。

## 过滤结果

| 候选 | 来源证据 | 本仓核验/在途判断 | 下一迭代处理 |
|---|---|---|---|
| Sonar 全项目测试覆盖率 ≥80% | 用户本轮明确要求；`REQ-SONAR-COVERAGE-80-01` | 已登记 Active，但尚未达标；现有仅有 new-coverage/受限扫描证据不足 | **纳入硬目标**：真实全项目 Sonar scan + Quality Gate/API，coverage ≥80.0；未达标不得宣称完成 |
| Agent communication/message hub 的完整任务、事件、控制、artifact 语义 | 基线来源 `9516749e-c317-4f13-9cda-b64b00cec465`；对话 `a47d3199-c1f9-47f1-927c-ff2c4875b77d` 第 10 轮 | `REQ-RIDGE-MCP-AS-KERNEL-API-01` 与 `REQ-AGENT-COMMUNICATION-REGISTRY-01` 已覆盖内核/API/registry；完整 hub residual 需复核 | 纳入候选 residual；先定边界与最小契约，不重复造第二身份源 |
| 临时 MCP/NotebookLM bridge 安全与幂等导入 | 临时来源 `df4d5dcc-9813-4c61-ae9f-1e9199cb7555` | 本仓主要通过本地 skill/专用 Chrome/固定代理运作，尚未形成 wind 产品 bridge；不能以文档代替代码证据 | 纳入候选审计项；若无本仓实现则记录为流程约束/外部依赖，不造伪实现 |
| RidgeCode 原生物理 scrollback、Lean Output、LifecycleAnchor、KKP、grapheme/断点 TUI | RidgeCode 27 来源、note 与对话 `68791fb7-659a-4ad6-a86c-beb7ac694781` | CodeGraph 在本仓未找到 `LifecycleAnchor`、`insert_before`、`DECSTBM`、`ClickRegionRegistry` 等核心符号；疑似另一 RidgeCode 项目，不得直接改 wind | 纳入审计结论；若确认跨仓，转外部 backlog，不算 wind 未落地需求 |
| Agent 视频工作室 | 视频研究来源/note/chat | 与 wind 当前产品边界无对应本仓路径或 Active REQ | 明确 out-of-scope |

## 验收底线

1. 每项候选必须有来源/对话/note ID、CodeGraph 符号或“本仓无对应实现”证据，并标明 Active REQ、残余、外部或 out-of-scope。
2. Sonar 目标按全项目 coverage 计，不以 restricted scan、new coverage 或本地估算替代；需保存 scan、Quality Gate 与 API 结果。
3. 临时来源只允许强化安全边界与审计，不得导出 cookie/token，不得向 NotebookLM 写入/删除/上传 source/note。

## 本轮完整只读盘点补充

- 主 Ridge notebook 的最近对话仍为 `a47d3199-c1f9-47f1-927c-ff2c4875b77d`（10 turns）；其近期痛点含 `max stall`、历史滚屏/原生 scrollback、接管提示与 `/help` 入口。CodeGraph 未在本仓找到对应 TUI 符号，故不把 RidgeCode 独立项目的结论移植到 wind。
- `RidgeCode现状、愿景、路线图` 的 27 个来源及 `68791fb7-659a-4ad6-a86c-beb7ac694781` 对话主要指向另一套 `crates/agent` TUI；`LifecycleAnchor`、`DECSTBM`、`ClickRegionRegistry` 等本仓无对应实现，判为外部 backlog。
- `Agent 视频工作室` 与 `nlm-mcp live deep research` 不属本次 pane/ridge-term/workspace/remote 代码边界；保留其安全约束为流程参考，不造 wind 伪实现。
- 工作流 notebook 对话提出的 `REQ-009`（质量遥测自动唤醒）与 `REQ-010`（Jules 环境自举）在 NLM 回答中仍标为 `[PENDING]`，当前本地 Active requirements 无对应批准链，暂不实施。
- 本仓 Git 发现契约已由 CodeGraph 复核并有确定性测试：`find_git_repos_below` 固定深度 `<=1`、直系子仓可发现、孙级仓不可发现、`.git` 边界不下钻；`paneGitStatus` 的非 Git cwd 不显示 descendant branch pill。Rust `scan_tests` `6/6`、`paneGitStatus.test.ts` `13/13` 通过。

NLM 本轮仍只作候选来源；未写入、删除或上传 NotebookLM source/note，以上 out-of-scope/pending 项不计入当前完成度。

## 新一轮 NLM 提问（只读）

- 已用固定代理完成 `refresh_auth` 后再次提问；`notebook_query` 在未显式提供 conversation 时仍复用最近活动会话 `a47d3199-c1f9-47f1-927c-ff2c4875b77d`，未产生独立新 conversation。该行为已记为工作流限制，不冒充“新对话”成功。
- NLM 返回 5 项：深根内核生命周期、Message Hub 强类型消息、非整数 DPR 原生对照、移动 PWA 后台自愈、Sonar Quality Gate。前 1/3/4 与 Active requirements 或既有物理证据缺口相交；Message Hub 仍属架构 residual/pending；Sonar 属本轮硬闸，不能归为 out-of-scope。
- NLM 所称“缺少 `SONAR_TOKEN`”与本地事实不完全相符：本机 `admin/admin` 登录可用，临时 token 生成/撤销链已验证；当前阻塞是新扫描上传/服务端指标闭合及覆盖率目标，不把 NLM 解释当根因。
- 新回答未写回 source/note；未新增 Active requirement，未执行 Message Hub、深根或 PWA 的 speculative code change。
