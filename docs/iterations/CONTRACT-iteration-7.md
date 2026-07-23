# CONTRACT — iteration 7

日期：2026-07-23
主题：证据与固化轮（冻结新产品功能）——loader 修复 + 弱网实验室 harness + 审计与文档固化

## 双轨制

**自动轨**（本合同验收对象）与**用户轨**（用户必办件，不计入自动验收）分列；自动轨不得以模拟数据冒充用户轨结论。

## 自动轨目标

### G1[T1·主线] 修复 `cargo test -p ridge --lib` 宿主载败

- 定位 `STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)` 根因（候选面：tauri 静态链接 + 系统 DLL 符号快照、link.exe 增量产物、杀软/加载器干预）。
- 验收：`cargo test -p ridge --lib` exit 0（连带首次执行 teammate 投影脱敏测试、hello 能力交集等全部 lib 测试）。
- 停机：确证 OS/环境级不可修 → 写明根因与规避文档，并把安全关键 lib 测试（投影脱敏）迁至可执行目标（bin/integration），不留「只编译不执行」的安全断言。

### G2[R1] 确定性弱网实验室 harness

- 扩展 fault-injection 场景族为参数化扫描（重连延迟档位 × 分片丢弃率档位 × 背压水位），输出 `artifacts/weak-net-lab/metrics.json`（gitignored）+ 生成脚本 `scripts/run-weaknet-lab.mjs`。
- 显式标注「实验室确定性模型，非真机结论」；指标含每场景恢复步数/重复恢复计数/丢帧后重同步次数。
- 验收：脚本 exit 0 且输出过 JSON 结构校验；场景全绿。
- 停机：任一场景暴露可复现缺陷 → 本轮转单缺陷修复。

### G3[A1] Teammate 结构冗余字段审计

- rustc dead_code + 全仓 grep 双证列出 `ridge_core::Teammate` 及相关结构未被消费的字段；确凿者删（每处独立提交），存疑者只报告。
- 验收：审计报告 + `cargo test --workspace --exclude ridge` 维持 882+ 绿。

### G4[docs] 固化与用户必办件单页

- `docs/plans/user-verification-checklist.md`：汇总四件用户必办（真机 smoke、生产 status 实跑、wind/ridge-cloud 两分支审查合并、Team 面板人工核验），每件给命令/runbook 链接与「完成证据形态」。
- `docs/README_CN.md` 或相应用户文档补 Team 面板与 capability 协商说明一段；`docs/WORKFLOW.md` 补双轨制一句。

## 用户轨（不计入自动验收）

1. iOS Safari + Android Chrome 真机 smoke（runbook：`docs/plans/cloud-remote-physical-smoke-runbook.md`）。
2. `node scripts/check-prod-status.mjs --base-url <生产域名>`（带 `RIDGE_ARTIFACT_TOKEN`）实跑留证。
3. 审查合并 wind `codex/remote-git-diff-iteration-1` 与 ridge-cloud `codex/remote-artifacts-status`；后者部署后 status 端点上线。
4. Remote 连一次核验 Team 面板（标签出现、roster 正确、点按切 pane）。

## 自动验收

```powershell
cargo test -p ridge --lib          # G1 目标信号（当前红）
node scripts/run-weaknet-lab.mjs   # G2，exit 0 + JSON 校验
cargo test --workspace --exclude ridge
pnpm exec vitest run packages/remote/src/shared/
pnpm exec svelte-check --tsconfig ./tsconfig.json --incremental --output machine --threshold error
```

## 明确不做（冻结清单）

- 不做 P2 HITL（含只读展示；`list_hitl_pending` 方案已归档待 P2 轮启用）。
- 不做 G1 暂停/恢复、M1/M2、H1、E1/E2；不加任何新 Remote 能力或协议面。
- 不以实验室 harness 数据宣称真机/生产结论。
