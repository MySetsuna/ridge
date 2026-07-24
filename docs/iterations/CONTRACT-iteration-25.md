> **SUPERSEDED for AC4 accounting** — 原拆账不计 10 分。诚实 credit 见 `CONTRACT-iteration-40`…`49` 与 `{SCRATCH}/ac4-ledger.md`。
# CONTRACT — Iteration 25（约 2 日 · OP-TERM-LINK）

## 范围边界

仅终端链接发现/下划线/点击仲裁；不改 hosts 出站。

## 目标（6）

1. decideHoverUnderline：仅 Ctrl+hit 显示下划线。
2. decideLinkClick：TUI on 单击给程序；Ctrl+hit 开链。
3. parsePathWithLocation / resolveOpenTarget。
4. resolvePathAgainstCwd。
5. manager 接线 dataset.linkUnderline + decide*。
6. vitest 全矩阵。

## 验收

| # | 信号 |
| --- | --- |
| 1 | linkAffordance.test.ts 全绿 |
| 2 | manager 含 decideHoverUnderline / decideLinkClick |

## 代码面

`linkAffordance.ts`+test、`manager.ts` pointer 路径。

