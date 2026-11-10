---
id: L4-TERMINAL-VIEWPORT-REPAINT
level: L4
title: 共享终端视口结构变更与同帧重绘
status: LOCKED
lifecycle: ACTIVE
parent: L3-TERMINAL-VISUAL-INPUT-REPAIR
code_targets:
  - packages/remote/src/shared/terminal/manager.ts
test_targets:
  - packages/remote/src/shared/terminal/manager.test.ts
---

# 共享终端视口结构变更与同帧重绘

Pane 的共享表面视口发生偏移或尺寸变化时，旧绘制区域须经现有 host 失效机制清理，所有仍可见 pane 在同一 compositor 帧得到重绘。几何相同的重复计算保持零额外表面清理。失效标志应在本帧队列处理和几何更新完成后消费，避免本帧已清屏却仍按旧 dirty 集合遗漏邻接内容。

用既有 manager fixture 覆盖共享网格收缩和平移、重复相同几何以及 dirty 收集过程中触发失效。公网复验真实终端输出、主机重连、不同尺寸控制方及画布残影区域；区分终端内核数据、命令落盘和实际画面证据。运行 npm test、lint、typecheck 与 SpecTree verify。
