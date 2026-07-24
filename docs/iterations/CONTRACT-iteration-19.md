# CONTRACT — Iteration 19（Remote / multi-host / agent 监控 / mobile）

## 目标

1. 库存 dual-end / multi-host / agent panel / mobile touch 代码事实。  
2. 修移动端滑屏 TUI 与 mouse release 对齐。  
3. Remote Team 面板 orch health + Suspended 可见。  
4. NLM open=0，来源恒 PROJECT-STATE。

## 验收

| # | 信号 |
| --- | --- |
| 1 | checklist open=0 |
| 2 | mobileTouchScroll 测 + capability/allowlist 绿 |
| 3 | hosts / orch_health cargo 绿 |
| 4 | nlm 仅 PROJECT-STATE；note `[已实现]` |

## 停机

闸红；或引入第二常驻源。
