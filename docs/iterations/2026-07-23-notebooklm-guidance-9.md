# NotebookLM 指导 — iteration 9 之后（规划 iteration 10）

日期：2026-07-23 · 来源：单一来源 `PROJECT-STATE`（source `0c58fdb2`，iteration 9 后版本）

## NotebookLM 原文要点

- Q1：主线 = **A1 close/rename 同源化 + LAN 漏广播缺陷修复**（价值最高：修「LAN 关区不通知他端」实缺陷；向内求精不扩协议面）。
- Q2：H1 **降级为待需求证据重开**（类 E2），代码保留不删。
- Q3：确立「审查包强制刷新」节律——每轮闭环必跑 `generate-review-pack.mjs`；用户轨消化前不启动 P2 阶段 2 等重型协议变更。
- Q4：M1 **门槛已到**（暂停/审批状态进程态重启即失，冲突「可续」愿景）→ 目标 2：`WorkspaceMemory` 结构体 6 字段 + serde 测试，不做持久化。
- 五目标：A1 主线（「Vitest 模拟 LAN 关闭验广播」）；M1 struct；H1 簿记；C1 五缺口人工判定文档化；审查包刷新。

## 对抗评审（代码事实 checker）

### 采纳

- **Q1 A1 主线**：✔ 与 iteration 9 审计发现直接衔接；三副本→`close_workspace_core`/`rename_workspace_core`（模式照抄既有 `create_workspace_core`），LAN 副本漏广播随收敛自然修复。
- **Q2 H1 降级**：✔（类 E2 簿记）。补充代码事实：`hosts/mod.rs::HostStatus` 的 `Connecting/Connected/Error` 变体 rustc 报 never constructed——随降级**容忍保留**（H1 重开时即用），不做删除翻腾。
- **Q3 节律**：✔ 闭环工序固化：每轮 step 9 前重跑审查导读；WORKFLOW.md 补一句。
- **Q4 的论据**（暂停/审批态重启即失 vs 可续愿景）：✔ 成立——但结论修正见驳回 2。
- **C1 逐缺口判定**：✔ 采纳；实现修正：报告为脚本生成（手改无效），判定落**脚本内 `JUDGMENTS` 静态表**再生成，保单源可重跑。

### 驳回（附代码事实）

1. **「Vitest 模拟 LAN 关闭验广播」——层错**：LAN `close_workspace` 在 `remote_host_impl.rs`（Rust），vitest 是 TS 门禁。正确验收 = cargo 测试：订阅 `remote_structural_tx`（broadcast channel）断言 `WorkspacesChanged` 事件恰发；三调用方委托核实。
2. **M1 struct-only 三度来袭**：无消费者结构体 + serde 测试仍是死脚手架（同 iteration 8/9 驳回逻辑）。门槛论据成立 ≠ 该先写 struct——正确首步是**设计文档**（持久化落点：`.ridge` 文件 vs 独立 json、与既有 save/restore 的关系、6 字段语义、读写方是谁），实现随真实读写方同轮出现。
3. **H1「移除开发指针」验收**：语义模糊，收敛为 PROJECT-STATE 行状态变更一处。

### 结论

iteration 10 = A1 同源化+缺陷修复（主线）+ M1 设计文档 + H1 簿记降级 + C1 判定入脚本 + 审查节律固化，五目标；见 `CONTRACT-iteration-10.md`。
