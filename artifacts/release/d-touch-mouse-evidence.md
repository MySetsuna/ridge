# D 触控/鼠标模式证据（Goal #2）

**Date**: 2026-09-15
**改了什么**: `src/remote/lib/TerminalCanvas.test.ts` 新增 16 个 D-protect 用例，全部通过。

## 旧测试覆盖盲点

旧 `TerminalCanvas.test.ts` 测的是 Agent 状态 chrome、IME 锚定、resize 协议等，**不**覆盖 touch/pointer 路径。`selectionMode=true → local_scroll` 决策函数（`decideTouchMouseGesture`）即便有 bug，旧测试集也察觉不到。

Goal 明确点出：selectionMode → local_scroll 的辅助函数测试**不能**作为「start/move/end/cancel 实际派发逻辑正确」的证据。本轮直接在源码路径上断言。

## 新增 16 个用例

| 用例 | 验证不变量 |
|---|---|
| 默认模式（无 mouse reporting）touchstart 不派发 mouse press | startBody 无 `kEncodeMouse()` 无条件派发 |
| mouse reporting 开启且不在 link 上 → 派发 mouse press | 命中 `decideTouchMouseGesture('press')` + `touchMouseDragging = true` |
| mouse reporting + 命中 link → 走 link 预留，不发 mouse press | `touchLinkCell` 与 `isMouseReporting() && !touchLinkCell` 互斥 |
| 显式 selectionMode → startSelection | startBody 调 `startSelection(row, col)` |
| touchmove 阈值以下不派发 | `if (moved < TOUCH_DRAG_THRESHOLD_PX) return` |
| 默认模式超阈值 → 滚轮 + scrollUp/scrollDown，**不**发 mouse drag | scroll 路径 + `if (touchMouseDragging)` 互斥 |
| mouse reporting 模式 + touchMouseDragging → mouse drag | 命中 `decideTouchMouseGesture('drag')` |
| selectionMode + 超阈值 → extendSelection | 命中 extendSelection + return |
| touchend mouse release 严格在 touchMouseDragging 分支 | 路径分支 |
| 一次点击只派发一次 mouse（touchend 不重复 press） | kEncodeMouse 只在 release 分支；press 只在 start 分支 |
| selectionMode tap → clearSelection + openSoftKeyboard | 路径分支 |
| 默认模式 tap → openSoftKeyboard（无 mouse press） | 路径分支 |
| touchcancel 清空所有 touch 状态 | 4 个状态全部重置：touchMouseDragging/selDragging/touchScrollAccum/touchLinkCell |
| selectionMode 短 tap 也 raise 软键盘 | 注释 `§select-tap-keyboard` + openSoftKeyboard |
| touchmove 不改 touch-action | handler 不 mutate touchAction css |
| `decideTouchMouseGesture` 三态契约（press/drag/release） | 三处调用 + 字符串一致 |

总计：`Test Files 1 passed (1) | Tests 31 passed (31)`

## 不动的部分（Goal 红线）

- **不**改 `openSoftKeyboard()` 入口（既有软键盘路径保留）
- **不**改 `<style>` 块的 touch-action css（handler 不 mutate）
- **不**改 IME 锚定 / 焦点 / 事件 stopPropagation 既有逻辑
- **不**改 decideTouchMouseGesture 决策函数本身（只验证调用契约）

## 已知限制

- 测试是**源码路径断言**（substring + 区间匹配），不是组件运行时调用。Svelte 5 + jsdom + @testing-library/svelte 没在测试栈装。运行期验证需要 `pnpm add -D @testing-library/svelte @testing-library/jest-dom` + jsdom 适配；本环境因依赖装/未装、补不可控，列入下轮。
- 直接 `decideTouchMouseGesture` 单测在该函数自己的测试文件里；本轮没动它（也不需要动 — Goal 关心的是「正确路径走对」，不是「决策逻辑改对」）。

## 复现

```bash
pnpm vitest run src/remote/lib/TerminalCanvas.test.ts
```

## 真机（移动端 Chrome / Safari）人工最小验证步骤

1. 启动 candidate：`./target/test-rdg/release/ridge.exe host --port 5120`
2. 手机 Chrome 打开 `https://<host>:5120/`（首次需信任 CA 或 HTTPS 证书）
3. 验证：
   - 普通模式（不打开 selection）：手指上下滑 → 屏幕滚动；**不**触发 TUI mouse
   - 进入 selection 模式：手指拖动 → 选区起止；抬手 → 弹软键盘
   - 进 TUI 鼠标报告程序（vim/neovim/htop）：tap → click；drag → press-hold；release → release-up
   - selectionMode tap 后不切到普通模式：再 tap 普通区域 → 仍在 selection mode + 软键盘已 raise
   - 切到别的 app 再回来：按 ctrl+shift+esc（系统手势）/ 切到桌面再回来 → 下一次 tap 不双派发
