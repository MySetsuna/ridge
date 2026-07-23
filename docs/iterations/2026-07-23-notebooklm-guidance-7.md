# NotebookLM 指导 — iteration 7 之后（规划 iteration 8）

日期：2026-07-23 · 来源：单一来源 `PROJECT-STATE`（source `cc0c592b`，iteration 7 后版本）
查询：为 iteration 8 裁决主线并给合同草案；回答 §7 四个开放问题；每目标给差距 ID / 可判定验收 / 停机 / 减法。

## NotebookLM 原文要点

### §7 开放问题裁决

1. **Q1 主线**：P2 第一阶段 Remote HITL 只读展示（价值最高：增强「可见」；只读不触审批逻辑与 E2EE；为 G1/P2 二阶段建基座；成本低可逆性高）。对比：G1 设计成本不确定；A1 写路径重构在真机证据为空时风险过高。
2. **Q2 载荷**：投影脱敏模式——严禁向 Remote 投影原始 `args`；仅投影 `nonce`(引用 ID)、`risk_level`、`category`、`summary`、`requester_id`。
3. **Q3 冻结**：不设硬性提交数阈值，实施「功能集冻结」——本轮严禁增加任何非 P2/G1 范畴的新协议字段。
4. **Q4 M1/M2**：维持靠后，不前置（依赖 stable_id 可靠性；遵守 §6 优先级矩阵）。

### 合同草案（六目标）

1. [主线 P2] `get_hitl_pending()` 路由 + CapabilityMatrix 宣告；投影 JSON 100% 不含 `args`；Remote 显示待审批标签及风险摘要。减法：不做审批按钮。
2. [G1] 暂停/恢复状态机设计文档 + `AgentStatus` 枚举新状态（Suspended/Resuming）类型检查；停机条件涉 SIGSTOP/SIGCONT 模拟死锁。减法：不写业务控制代码。
3. [A1] pane.rs 只读 vs 写路径分类审计；「删除至少 2 处已确认陈旧路径」。减法：不做写路径同源化。
4. [S1] F3–F6 触发时 `telemetry.log` 准确产生计数。减法：不实现自动退役。
5. [T3] `check-prod-status.mjs` 在 CI 环境下 exit 0（桩数据）。
6. [T1] `cargo test --test boot_smoke` 持续通过。

## 对抗评审（codegraph/代码事实 checker）

### 采纳

- **Q1 主线 = P2 只读第一阶段**：✔ 与归档方案（iteration 6 驳回记录中 `list_hitl_pending` 待 P2 启用）一致；走既有 `teammate` 能力宣告纪律，不加新 capability。
- **Q2 核心 = 不投影原始命令文本**：✔ 代码事实强化此裁决——`hitl.rs` 事件载荷 `{id, initiator, action, level, reason}` 中 `action` 为命令全文（可含密钥），远端投影必须剔除。
- **Q3 功能集冻结**：✔ 本轮唯一协议面新增限 P2 范畴（`teammate` 能力下新增只读方法）。
- **Q4 M1/M2 靠后**：✔。
- **G1 设计先行、A1 审计先行、S1 不做自动退役**：方向均采纳（细节修正见下）。

### 驳回（附代码事实）

1. **臆造字段 `nonce`/`category`**：`hitl.rs`/`teammateModel.ts` 无此二字段。现有字段即够：`id`（引用）、`level`（RiskLevel）、`reason`（分类器人话理由，即其所谓 summary）、`initiator`（请求方）。nonce/单次消费属裁决通道（第二阶段），只读列表无需。
2. **关键遗漏（NotebookLM 未见）**：`PENDING: HashMap<String, oneshot::Sender<HitlResolution>>` **只存 id→sender，元数据发事件即弃**——`list_hitl_pending` 前提是扩注册表值存脱敏元数据 `{initiator, level, reason, created_at}`（同一注册表加宽，非新状态源）。合同已补此步。
3. **`telemetry.log` 持久面**：违反 iteration 5 锁定的遥测设计（进程内计数、无新持久面）与「日志不得输出敏感」不变量。维持 F1/F2 计数器模式。另按矩阵：F5（`keyBindingVerifier` 生产未接）的退役条件本就是「确认冗余则**删除**」——计数不如删除审计。
4. **`AgentStatus` 枚举加 Suspended/Resuming**：与其自己的减法（不写业务代码）矛盾——无实现的枚举变体是死状态入产品枚举，违 YAGNI；G1 本轮止于设计文档。其「严禁 wind 建 AgentStatus 本地副本」系把云协议 SSOT 不变量张冠李戴到 wind 内部枚举。「SIGSTOP/SIGCONT 模拟」为 Unix 中心视角——Windows 无 SIGSTOP，设计文档须给 Windows 替代（job object 冻结 / stdin 门控 / PTY 暂停）。
5. **A1「删除至少 2 处」配额**：审计前承诺删除数无据——iteration 7 Teammate 审计即零死字段先例。验收改「报告 + 确凿者删（可为零）+ 门禁维持绿」。
6. **T3 CI 桩挂钩**：本仓无 CI（NotebookLM 屡次臆造）；桩验四径 iteration 5 已做，零新价值。
7. **T1 boot smoke「持续通过」**：`tests/win_manifest_boot.rs` iteration 7 已交付且随 workspace 门禁运行；「保持绿」非目标。

### 结论

iteration 8 = P2 只读主线 + G1 设计文档 + A1 pane.rs 审计 + S1 F3/F4 计数与 F5 删除审计，四目标；见 `CONTRACT-iteration-8.md`。
