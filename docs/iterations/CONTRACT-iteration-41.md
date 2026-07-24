# CONTRACT — Iteration 41 / AC4-C2（约 2 日 · TERM-LINK）

**Credit ID**: C2

## 产品结果

Ctrl-hover 可见下划线；TUI mouse on 单击归程序；Ctrl+click 开链/路径；路径 cwd 解析。

## 独占主文件

- `packages/remote/src/shared/terminal/linkAffordance.ts`(+test)
- `packages/remote/src/shared/terminal/manager.ts`（接线 hunks）

## 验收

| # | 信号 |
| --- | --- |
| 1 | vitest linkAffordance → `{SCRATCH}/gates-credit-C2.log` |
| 2 | manager 引用 decideHoverUnderline / decideLinkClick |
