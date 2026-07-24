# 开放愿景清单 — Explorer 文件树 free-follow 压缩下方展示域

更新：2026-07-24  
NLM 指导 note：`Ridge 布局交互重构与动态压缩方案`（36307def…）

| ID | 主题 | 状态 | 根因 / 证据 |
| --- | --- | --- | --- |
| **EX-FREE-FOLLOW** | 分隔条拖拽连续跟 `clientY`，不被下方面板 hit-test 卡住 | **implemented** | `window` pointermove + capture + `rg-explorer-resizing`；上界=`measureFreeFollowSpan`（栈顶→explorer 底，含后续 cwd） |
| **EX-COMPRESS-BELOW** | 下方/后续展示域随拖实时压缩，可越过原 header 上沿 | **implemented** | 栈 `flex-[0_1_auto]` 随 body 长高挤后续 cwd；有 pane 插件时 lower `min-h-0 flex-1` 被压 |
| **EX-NO-EMPTY-HALF** | 无 pane 插件时不 50/50 空分 lower | **implemented** | `resolveExplorerStackLayout`：`hasLowerContent=false` → `showLower=false`；默认 body `flex:1 1 0` |
| **EX-BODY-SHRINK** | 固定 H 可 shrink + live reclamp | **implemented** | `flex: 0 1 Hpx`；`ResizeObserver`→`reclampStoredBodyHeight` |
| **EX-PURE-MATH** | 高度/布局决策纯函数可测 | **implemented** | `explorerLayout.test.ts` 11 项 |

**open 计数：0**

## 明确不纳入

- 并行会话其他愿景  
- SplitContainer 终端分屏 Fluid Resize 全量重写  
- FileTree 文件移动 DnD  

## 门禁

`pnpm exec vitest run src/lib/stores/explorerLayout.test.ts` → `{SCRATCH}/explorer-resize-gates.log`
