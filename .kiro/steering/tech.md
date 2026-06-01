# 技术决策记录

## 终端内边距 (Terminal Padding)

- **默认值**: `terminalPaddingPx` 在 `src/lib/stores/settings.ts` 中设为 `0`（无内边距）
- **理由**: 终端内容应紧贴 pane 边缘，不再保留默认 2px 内边距。用户如需边距可自行调整。
- **影响路径**:
  - `RidgePane.svelte` 的 `$effect` 读取 `settingsStore.terminalPaddingPx` → 调用 `manager.setPadding(paneId, px)`
  - `manager.ts` 的 `fitPane()` 中 `basePad` 计算：`(entry.lastAppliedPaddingPx ?? this.opts.paddingPx) || 0`
  - `manager.ts` 的 `attach()`/`unpark()` 中 `this.opts.paddingPx` 本身为 `undefined`，不写入 CSS padding
  - `setPadding(paneId, 0)` → `container.style.padding = ''` → 清除所有内边距

## Split Pane 布局修复

- **问题**: split 后新 pane 的终端区域可能不铺满整个 pane（kernel 停留在默认 80×24）
- **原因**: `attach()` 是异步的，Svelte 挂载 + wasm 初始化 + DOM layout 结算需要多帧时间
- **修复** (`src/lib/stores/paneTree.ts` 的 `scheduleForceFitAfterSplit`):
  - 从固定 2-RAF 改为多时间点重试：~2 帧、50ms、150ms、400ms
  - `fitPaneNow` 本身在尺寸未变化时是 no-op，多次调用无副作用
  - 配合 ResizeObserver + 1000ms debounce 作为最终 fallback
