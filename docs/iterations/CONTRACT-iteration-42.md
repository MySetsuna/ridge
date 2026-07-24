# CONTRACT — Iteration 42 / AC4-C3（约 2 日 · Git + process_guard）

**Credit ID**: C3

## 产品结果

Git 唯一出口超时杀树可观测；共享 `process_guard`；双端 concurrency 常量交叉；AgentCenter git 护栏 badge。

## 独占主文件

- `packages/ridge-core/src/process_guard.rs`
- `packages/ridge-core/src/commands/git.rs`（计数/静态门禁/委托 kill）
- `src/lib/stores/gitGuardStats.ts`(+test)
- `src/lib/utils/pLimit.test.ts`（交叉 MIN/MAX）

## 验收

| # | 信号 |
| --- | --- |
| 1 | `cargo test -p ridge-core --lib process_guard guard_tests` → `{SCRATCH}/gates-credit-C3.log` |
| 2 | production_git_spawn_only_via_git_cmd 绿 |
| 3 | process_guard 挂起假二进制超时测绿 |
