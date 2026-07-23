# CONTRACT — iteration 10

日期：2026-07-23
主题：A1 workspace 写路径同源化 + LAN 漏广播缺陷修复（主线）+ M1 设计 + 簿记清偿

## 双轨制

自动轨如下；用户轨四件不变。**零 Remote 协议面变更**；不引入持久化实现（M1 止于设计）。

## 自动轨目标

### G1[A1·主线] close/rename 三副本同源化 + LAN 漏广播修复

- 抽 `close_workspace_core(&AppState, id) -> Result<(), String>`（含：最后一个拒、顺序表/映射移除、`workspace_names` 清理三方对齐、活动区改选、`WorkspacesChanged`+`WorkspaceListChanged` 双广播）；三调用方（桌面命令、`WorkspaceWriter` 端口、LAN `remote_host_impl`）全部委托——LAN 副本漏广播随之修复。
- 同法抽 `rename_workspace_core`（改名 + auto-save + 三广播），双调用方委托。
- 验收：`cargo test --workspace` exit 0；新 Rust 测试订阅 `remote_structural_tx` 断言 close/rename 经 core fn 恰发广播；`workspace_names` 清理行为测试钉死。
- 停机：收敛暴露锁序冲突（不同持锁顺序不可统一）→ 记录并保守回退该径。
- 减法：rename 若与 auto-save 耦合过深，本轮仅收敛 close（合同允许）。

### G2[M1] Workspace Memory 设计文档（零代码）

- `docs/superpowers/specs/2026-07-23-workspace-memory-design.md`：6 字段语义（目标/约束/决策/任务/运行状态/时间戳级别）、持久化落点选型（`.ridge` 文件段 vs 独立 sidecar json，与既有 save/restore 与 auto-save 的关系）、首批真实读写方（G1 暂停态恢复 + HITL 审批历史）、隐私边界（不落命令全文/密钥）。
- 验收：文档提交；零代码 diff。
- 减法：不写 struct、不写迁移（驳回 NotebookLM struct-only 三度提案）。

### G3[H1] 簿记降级

- PROJECT-STATE：H1 改「已关闭——待用户真实需求证据重开」；`hosts` 死变体容忍保留注记。
- 验收：状态文档更新；零代码。

### G4[C1] 逐缺口判定入脚本

- `scripts/rdg-gap-report.mjs` 增 `JUDGMENTS` 静态表：五 denied 能力逐项判定（theme=永久缺口（无 UI 宿主）；teammate=刻意排除已注；git/workspace=补路由候选（ridge-core dispatch 已共享，待需求触发）；invoke=按矩阵语义注记），重生成报告。
- 验收：脚本 exit 0；报告含判定列且无「待人工判定」残留。
- 停机：某缺口判定需改云端协议权威文档 → 该项记「需协议轮」不擅动。

### G5[节律] 闭环审查导读强制刷新

- `docs/WORKFLOW.md` 补一句：每轮闭环（step 9 前）必跑 `node scripts/generate-review-pack.mjs` 刷新导读。
- 本轮起执行：闭环提交前重跑。
- 验收：WORKFLOW 更新；导读覆盖至本轮全部提交（脚本自校验）。

## 自动验收

```powershell
cargo test --workspace
pnpm exec vitest run packages/remote/src/shared/
pnpm exec svelte-check --tsconfig ./tsconfig.json --incremental --output machine --threshold error
node scripts/rdg-gap-report.mjs
node scripts/generate-review-pack.mjs
```

## 明确不做

- 不扩 Remote 协议面；不做 P2 阶段 2 / G1 阶段二；不实现 M1 持久化；不删 hosts 存量面；不引入持久遥测/CI 假设。
