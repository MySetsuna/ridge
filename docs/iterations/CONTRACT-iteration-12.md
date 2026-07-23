# CONTRACT — iteration 12（收敛轮）

日期：2026-07-23
主题：自动轨存量收官——核验会动线 + 簿记归档 + 维护态定型。本轮后循环转**低频维护态**，直至用户轨产出首份证据。

## 自动轨目标

### G1[主线·用户轨解冻器] 30 分钟核验会动线文档

- `docs/plans/30-min-verification-session.md`：把用户轨四件 + 历轮顺带核验项（Team 面板含 Pending 区、暂停/恢复、暂停态跨重启、LAN 关区广播）串成单次动线（准备→桌面侧→Remote 侧→生产侧→合并审查），每步给精确命令/操作与证据形态、预计耗时。
- 验收：文档提交；覆盖 checklist 全部条目（对照无遗漏）。
- 减法：不做 evidence 打包脚本（YAGNI，各件证据形态已定义）。

### G2[簿记] G1 阶段二 / M1 余切片 / M2 归档

- PROJECT-STATE：G1 阶段二改「已关闭——待痛点证据重开」（软暂停已覆盖主场景）；M1 切片二/三与 M2 改「已关闭——待 P2 阶段 2 实现解冻后合并重开」；差距表进入终态快照。
- 验收：状态文档更新；零代码。

### G3[维护态定型] 节律与合同模板

- WORKFLOW.md 补维护态定义：验收 = 全门禁绿（cargo/vitest/svelte-check）+ 导读刷新 + 零回归；解冻条件 = 用户轨首份证据（真机 evidence JSON / 生产 status 实跑 / 分支合并任一）。
- 验收：WORKFLOW 更新；本轮门禁全绿 + 导读刷新执行。

## 自动验收

```powershell
cargo test --workspace
pnpm exec vitest run packages/remote/src/shared/
pnpm exec svelte-check --tsconfig ./tsconfig.json --incremental --output machine --threshold error
node scripts/generate-review-pack.mjs
```

## 明确不做

- 不启动任何新功能/协议面/状态源；不实现 G1 阶段二、M1 余切片、M2、P2 阶段 2（皆待证据/用户轨解冻）。
