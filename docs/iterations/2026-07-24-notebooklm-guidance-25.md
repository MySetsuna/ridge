# NotebookLM guidance + 对抗 — iteration 25

笔记本：`66919cb9-1329-4ddf-955c-f426d15a9fe6`  
来源：sole PROJECT-STATE  
本轮主题：**Terminal link hover underline + TUI click arbitration**

## Maker 建议（综合 PROJECT-STATE + 开放规划 Note）

1. 优先可确定性验收的 Ridge 切片，避免整包偏离北极星方案。
2. 本轮合同目标见 `CONTRACT-iteration-25.md`（4–8 项）。
3. 不引入新状态源 / 协议 SSOT 副本。

## 对抗评审 reframe 表

| 原建议 | 更高价值可测切片 | 验收信号 | 裁决 |
| --- | --- | --- | --- |
| 偏离北极星整包（VNC/daemon/CRDT/视觉大改等） | 整终端重写 → 纯函数仲裁 + Ctrl-hover 下划线切片 | vitest linkAffordance | **reframed 采纳** |
| 仅文档/空 Release | 禁止 | — | **non-goal** |
| 本轮合同内目标 | 按合同实现 | 编译器/测试 exit 0 | **采纳** |

## 锁定

- 代码面：`linkAffordance.ts; manager decideHoverUnderline/decideLinkClick`
- 不做简单 rejected 空转；物理真机仍用户轨 checklist。
