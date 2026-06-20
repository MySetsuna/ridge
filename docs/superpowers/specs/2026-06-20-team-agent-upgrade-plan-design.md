# Ridge Team-Agent 整体规划：底座化改造（瘦身 + 强化 + 一处新增）· 可立即落地

> 日期：2026-06-20
> 上游：`2026-06-19-domain-zero-teammate-design.md`（四 Domain 已落地：纯核心 286 测试 + 接线 + 前端）。
> 本文取代上一版"对标扩张"思路，定为**唯一权威执行计划**。

## 0. 一句话与分界线

**Ridge 不做"Agent Team 平台/编排器"，做"一个人安全地、就地指挥一群可见 panes 的终端底座"。**

切割线只有一条——**保留"给人用的"，砍掉"给 AI 之间自治用的"**：

| 给人用的（底座，留） | 给 AI 自治协同用的（脚手架，砍/冻） |
|---|---|
| 看见 panes（状态/告警）、拦住危险命令（HITL）、断开死循环（熔断）、接入任意 agent（MCP） | TML 互发线协议、广播抢单、AI 竞选 Leader、性格分派、拓扑 DAG |

## 1. 依据（压缩结论）

- 四簇 ~20 竞品勘探 + 趋势第一性判断：**紧耦合多 agent 协同是补当下模型短板的过渡脚手架，会被更强基座吃掉**（Cognition《Don't Build Multi-Agents》、Anthropic 多 agent 仅在可并行检索上划算且 ~15× token）。**并行 fan-out 会留下，深度协同会萎缩。**
- 穿越周期、云平台又抢不走的三样：**人在中间的高带宽监督 + 不可绕过的安全闸 / 跑在真实机器上的 locality / 不重复学习（文件化记忆）**。Ridge 已有前两样的地基。
- 校正：① Multica 的"解法自动变技能"**未发货**（自动蒸馏在研究界已成熟，Ridge 可反超）；② worktree 隔离不免费（撞环境/端口），Ridge"共享工作区+写锁"对真协作更对路 → 隔离只做可选档，**本计划不含**。

## 2. 处置总表（留 / 冻 / 删，含文件落点）

### ✅ 留下并强化 —— 底座
| 组件 | 文件 | 动作 |
|---|---|---|
| 🛡️ HITL + 风险分级 | `ridge-core/teammate/risk.rs`、`src-tauri/teammate/hitl.rs`、`commands/teammate.rs`(set_hitl_enabled/classify_command_risk)、`HitlApprovalModal.svelte` | **招牌#1**：补图形开关、做成"不可整体关" |
| 🔌 MCP server | `ridge-core/mcp/*`、`server.rs`(/api/v1/mcp/ws) | **招牌#2**：开放底座，持续投入 |
| ⚡ 熔断（哑保险丝） | `ridge-core/teammate/circuit_breaker.rs`、`src-tauri/teammate/circuit.rs` | 保留现状；**不**升级双账本/重规划（那是编排器） |
| 👁️ 状态 + 告警 | `AgentCenterPanel.svelte`、`agentCenter.svelte.ts` | 收缩为 Roster + 熔断告警两块 |

### 🧊 冻结降级 —— 已建、gated-off，停止投入、不删不露出
| 组件 | 文件 | 动作 |
|---|---|---|
| 写锁 | `ridge-core/teammate/write_lock.rs`、`src-tauri/teammate/locks.rs` | 低成本安全网，冻结；别长成"Diff 仲裁视图" |
| delegate / report / team-profile | `server.rs` 高层 API | 保留机制，语义改为**人发起**（"把任务发给那个 pane"）；report 只喂保险丝 |
| 名册数据 | `ridge-core/teammate/model.rs`(精简 Teammate)、`src-tauri/teammate/profiles.rs` | 保留"谁是 agent/busy/idle"，去掉竞选用法 |

### ❌ 摒弃 —— 押在会萎缩的"AI 自治"层，违背极简、与 Claude Code 重复且更脆弱
| 组件 | 文件 | 动作 |
|---|---|---|
| TML 协议 + StreamCleaner | `ridge-core/teammate/tml.rs`、`ridge-core/teammate/stream_cleaner.rs`、`src-tauri/teammate/stream.rs`、`engine::pty::spawn_pty_reader` 的 cleaner 钩子、`set_tml_stream_enabled` | 删 |
| 广播抢单 | `server.rs` broadcast 路由 | 删 |
| Leader 竞选 + 性格分派 | `ridge-core/teammate/topology.rs`(elect_leader)、`model.rs`(Personality) | 删竞选/性格；topology 收缩为扁平 roster 或并入 model |
| DAG / Objective / 协作审计 | `AgentCenterPanel.svelte` 三区 | 删 |

### ➕ 唯一新增 —— 终端原语「会话/工作永不丢」
持久化做成 append-only 日志（符合极简、与 scrollback 同气质，不依赖任何被摒弃层）。**技能复利仅以 `.ridge/skills/*.md` 文件约定起步、列入观望，不在本计划内建子系统。**

## 3. 落地步骤（5 个 commit，按风险升序，可立即开工）

> 前提：本机 develop 多会话共 tree——每个 commit 前先 `git status`/核对 HEAD，热点文件（`SplitContainer.svelte`/`server.rs`/`state.rs`）按 hunk 隔离。提交一事一 commit。

### Commit 1 — 冻结（前端隐藏 + flag 永关，零删除、零风险、先落）
- `AgentCenterPanel.svelte`：隐藏 **DAG / Objective / 协作审计** 三区，仅留 Roster + 熔断告警。
- `set_tml_stream_enabled`/StreamCleaner：确保无路径再打开，代码与文档标 `deprecated`。
- `docs/Agent-Team协同使用手册.md` §1/§5/§6：自治协同叙事降级。
- **验证**：`pnpm check` 0/0 + `vitest` + `tauri:dev:cdp` 看 Agent Center 收缩。**不杀会话。**

### Commit 2 — 摒弃（cleanup，删脚手架 + 对应测试）
- ridge-core：删 `tml.rs`、`stream_cleaner.rs`、`topology.rs::elect_leader` + `model.rs::Personality`（topology 收缩为扁平 roster）。
- src-tauri：删 `stream.rs` 与 `spawn_pty_reader` cleaner 钩子、`server.rs` broadcast、`commands/teammate.rs::set_tml_stream_enabled`；`circuit.rs` report-progress 去掉 leader 语义。
- 删对应单测（测试数会掉，掉的是脚手架的，正常）。
- **验证**：`cargo test -p ridge-core`（不杀会话）+ `cargo check -p ridge`（标**待 rebuild + 真机 e2e**）。

### Commit 3 — 强化招牌 HITL（图形开关 + 不可整体关）
- 设置面板加 HITL 可视开关（替代命令式 `set_hitl_enabled`）。
- 审计无"一键关安全"旁路；学 Claude Code"hooks 即便 yolo 也触发"——闸不可被整体关。
- **验证**：`pnpm check` + `vitest` + `tauri:dev:cdp` 走 L2 命令弹审批。

### Commit 4 — 新增持久化原语（会话永不丢）
- `ridge-core/teammate/journal.rs`：append-only 事件类型 + 重放（纯结构，可单测）。
- `src-tauri/teammate/store.rs`：落 `.ridge/teammate/journal.sqlite`（`rusqlite` 已在依赖）；写入异步、失败降级内存态（零默认行为变化）。
- 启动重放重建 roster / HITL pending。
- **验证**：ridge-core 重放单测 + `tauri:dev:cdp` 重启后状态仍在。

### Commit 5 —（观望，可不做）技能文件约定起步
- 仅约定 `.ridge/skills/<slug>/SKILL.md` + `mcp/resource.rs` 暴露只读 `ridge://skills/*`。
- **不做**蒸馏、不做 sqlite 索引、不做向量库。按真实使用再决定是否长出。

## 4. 验证矩阵
| 层 | 手段 | 杀会话? |
|---|---|---|
| ridge-core（删脚手架 / journal / risk / circuit） | `cargo test -p ridge-core` | 否 |
| 前端（Agent Center 收缩 / HITL 开关） | `pnpm check` + `vitest` + `tauri:dev:cdp` | 否 |
| src-tauri 接线（store / cleaner 摘除 / broadcast 删） | `cargo check` + 标待 rebuild + 真机 e2e | 是（正式版）/否（dev:cdp） |

## 5. 护城河 + 观望清单
- **必须保住并强化**：L0/L1/L2 + 不可绕过 HITL；熔断 + 写锁；端侧数据不出机；同屏就地高带宽监督。
- **观望（基座/真实使用逼出形态再说，现在一律不做）**：fan-out-then-judge、opt-in worktree 隔离档、多厂商 provider 抽象、双账本/co-planning、技能自动蒸馏、看板。

---

*依据来源：竞品四簇勘探（终端跑手 Claude Squad/uzi/vibe-kanban/Conductor/Crystal/container-use/Sculptor；编排 [Magentic-One](https://arxiv.org/html/2411.04468v1)/[Magentic-UI](https://www.microsoft.com/en-us/research/publication/magentic-ui-report/)/[LangGraph](https://docs.langchain.com/oss/python/langgraph/overview)/[OpenHands](https://docs.openhands.dev/openhands/usage/architecture/runtime)/MetaGPT/Swarm/CrewAI；在售 [Warp](https://www.warp.dev/blog/reimagining-coding-agentic-development-environment)/[Cursor](https://cursor.com/changelog/2-0)/[Zed](https://zed.dev/blog/parallel-agents)/[Devin](https://cognition.ai/blog/devin-2)/[Augment](https://www.augmentcode.com/product/remote-agents)/[Multica](https://github.com/multica-ai/multica)；技能/记忆 Claude Skills/Cline/Letta/Voyager/ExpeL/AWM）。趋势判断参考 Cognition《Don't Build Multi-Agents》、Anthropic 多 agent 研究系统成本结论。*
