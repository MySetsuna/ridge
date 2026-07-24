# CONTRACT — Iteration 52 / AC4-C52（约 2 日 · 进程护栏策略双端）

**Credit ID**: C52 · OP-GIT-BYPASS 加厚

## 产品结果
processGuardPolicy 前端压力模型、双端 GIT_CONCURRENCY 常量、spawn registry 策略行、process_guard 超时杀树回归。

## 独占主文件
- `src/lib/stores/processGuardPolicy.ts`(+test)
- `packages/ridge-core/src/external_spawn_registry.rs`
- `packages/ridge-core/src/process_guard.rs`
- `src/lib/stores/gitGuardStats.ts`

## 验收
vitest processGuardPolicy；cargo process_guard + external_spawn_registry。

## 停机
前端 abort 冒充杀进程。
