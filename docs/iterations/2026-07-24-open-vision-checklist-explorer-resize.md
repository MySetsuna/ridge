# 开放愿景清单 — Explorer 文件树 free-follow 压缩下方展示域

更新：2026-07-24  
NLM 指导 note：`Ridge 布局交互重构与动态压缩方案`（36307def…）

| ID | 主题 | 状态 | 根因 / 证据 |
| --- | --- | --- | --- |
| **EX-FREE-FOLLOW** | 分隔条拖拽连续跟 `clientY`，不被下方面板 hit-test 卡住 | **implemented** | `window` pointermove + `setPointerCapture` + `body.rg-explorer-resizing` 屏蔽 `.explorer-lower` pointer-events |
| **EX-COMPRESS-LOWER** | 下方展开展示域随拖实时压缩，可越过原 header 上沿 | **implemented** | `.explorer-col-stack` 真 flex 列；`.explorer-lower` `flex:1; min-h:0; overflow:auto`；`clampBodyHeight` max=`col−sep−minLower` |
| **EX-PURE-MATH** | 高度分配纯函数可测 | **implemented** | `explorerLayout.{clampBodyHeight,computeBodyHeightFromDrag,lowerRegionHeight}` + `explorerLayout.test.ts` |

**open 计数：0**

## 明确不纳入

- 并行会话其他愿景  
- SplitContainer 终端分屏 Fluid Resize 全量重写  
- FileTree 文件移动 DnD  

## 门禁

`pnpm exec vitest run src/lib/stores/explorerLayout.test.ts` → `{SCRATCH}/explorer-resize-gates.log`
