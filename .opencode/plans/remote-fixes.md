# Remote Page 三大问题修复计划

## 问题 1: WebGPU 渲染黑屏

### 根因分析

Remote 端的 `terminalController.ts` 创建了 `SurfaceHostHandle` 并传给 `RenderHandle.newWithWebgpuFirst()`，但渲染循环 `tick()` 只调用了 `this.renderHandle.render(this.kernel)`，**从未调用 `beginFrame()` / `endFrame()`**。

桌面端 `manager.ts` 的 RAF 循环正确执行了三步协议：
1. `activeHost.beginFrame(themeBg)` — 获取 swap chain texture
2. `handle.render(kernel)` — 录制绘制命令
3. `activeHost.endFrame()` — submit + present

Remote 端缺少步骤 1 和 3，导致 WebGPU 命令被录制但从未提交到 surface，结果黑屏。

### 修复方案

**文件**: `src/remote/lib/terminalController.ts`

1. 在 `TerminalController` 类中存储 `SurfaceHostHandle` 引用（新增 `private surfaceHost` 字段）
2. 新增 `private themeBg: Uint8Array` 字段（4字节 RGBA，从主题派生）
3. 在 `applyTheme()` 中更新 `themeBg`
4. 在 `tick()` 中，如果 `surfaceHost` 存在：
   - 渲染前调用 `surfaceHost.beginFrame(themeBg)`
   - 渲染后调用 `surfaceHost.endFrame()`
   - 如果 `beginFrame` 返回 false（surface lost），跳过渲染

---

## 问题 2: Tab UI 改为 Select Dropdown

### 2a: 工作区标签 → 始终使用 Select Dropdown

**文件**: `src/remote/TopBar.svelte`

将 `.ws-tabs` 区域替换为一个 `<select>` 元素：
- 选项显示工作区名称
- 选中项绑定 `activeWorkspaceId`
- 保留关闭按钮（当工作区数量 > 1 时，在 select 旁边显示一个关闭按钮）

### 2b: 终端标签 → 响应式切换

**文件**: `src/remote/TopBar.svelte`

- 宽度足够时：保持当前内联按钮样式
- 宽度不足时（标签溢出）：折叠为 `<select>` dropdown
- 实现方式：
  - 新增 `$state` 变量 `paneTabMode: 'inline' | 'select'`
  - 使用 `ResizeObserver` 监听 `.pane-tabs` 容器的宽度
  - 当 `scrollWidth > clientWidth` 时切换到 select 模式
  - 保留 "+" 新建按钮在 select 旁边

---

## 问题 3: 桌面端与 Remote 端状态实时同步

### 根因分析

当前架构中：
- Remote 创建/关闭 pane/workspace → 服务端处理 → Remote 自行调用 `listPanes()` 刷新 → **桌面端不知道**
- 桌面端创建/关闭 pane/workspace → Tauri command → 桌面 Svelte store 更新 → **Remote 端不知道**
- 工作区重命名 → 桌面端有，Remote 端协议中缺失

需要双向推送结构变更（元素增删、命名），仅"当前选中状态"保持分离。

### 3a: Server → Remote 推送（广播通道）

**Rust 端修改**:

1. **`src-tauri/src/state.rs`**: 新增 `remote_broadcast_tx: broadcast::Sender<RemoteStructuralEvent>` 到 `AppState`
   - `RemoteStructuralEvent` 枚举: `PanesChanged { ws_id }`, `WorkspacesChanged`, `WorkspaceRenamed { ws_id, name }`

2. **`src-tauri/src/remote/server.rs`**: 
   - 在 WS handler 的 `tokio::select!` 中新增一个分支，监听 `broadcast::Receiver`
   - 收到 `PanesChanged` 时，自动发送 `list-panes` 响应给该客户端
   - 收到 `WorkspacesChanged` 时，自动发送 `list-workspaces` 响应
   - 收到 `WorkspaceRenamed` 时，发送新的 `workspace-renamed` 消息

3. **在结构变更点触发广播**:
   - `create-pane` handler → `PanesChanged`
   - `close-pane` handler → `PanesChanged`
   - `create-workspace` handler → `WorkspacesChanged`
   - `close-workspace` handler → `WorkspacesChanged`

**TypeScript 端修改**:

4. **`src/remote/lib/wsRemote.ts`**: 
   - 新增 `workspace-renamed` 消息类型到 `WsMessage`
   - 在 `onMessage` handler 中处理

5. **`src/remote/MainApp.svelte`**: 
   - 在 `onMessage` 回调中监听 `panes` / `workspaces` / `workspace-renamed` 消息
   - 当收到非请求触发的 `panes` 消息时，直接更新 `panes` 状态
   - 当收到 `workspace-renamed` 时，更新对应工作区名称

### 3b: Remote → Desktop 推送（Tauri 事件）

**Rust 端修改**:

1. **`src-tauri/src/types.rs`**: 新增 `GlobalEvent` 变体:
   ```rust
   PaneTreeChanged { workspace_id: Uuid },
   WorkspaceListChanged,
   ```

2. **`src-tauri/src/remote/server.rs`**: 在 `create-pane` / `close-pane` / `create-workspace` / `close-workspace` handler 中，通过 `state.event_tx` 发送对应事件

3. **`src-tauri/src/lib.rs`**: 在主事件循环中处理新事件，emit Tauri 事件到前端

4. **桌面端前端**: 监听 `pane-tree-changed` / `workspace-list-changed` 事件，刷新 `paneTreeStore`

### 3c: Desktop → Remote 推送

这已经由 3a 覆盖：桌面端的结构变更通过 Tauri command 修改 `AppState`，需要在这些 Tauri command 中也触发 `remote_broadcast_tx`。

**修改点**:
- `src-tauri/src/commands/pane.rs`: `split_pane`, `close_pane` 等函数中发送 `PanesChanged` 广播
- `src-tauri/src/commands/terminal.rs`: 工作区相关操作中发送 `WorkspacesChanged` 广播
- 工作区重命名操作中发送 `WorkspaceRenamed` 广播

---

## 实施顺序

1. **Issue 1** (WebGPU 黑屏) — 最小改动，最高优先级
2. **Issue 2** (Tab UI) — 纯前端改动，独立于 Issue 3
3. **Issue 3** (状态同步) — 最复杂，涉及 Rust + TypeScript 双层修改

## 文件变更清单

| 文件 | 变更类型 | 所属 Issue |
|------|---------|-----------|
| `src/remote/lib/terminalController.ts` | 修改 | #1 |
| `src/remote/TopBar.svelte` | 重写 | #2 |
| `src-tauri/src/types.rs` | 新增枚举变体 | #3 |
| `src-tauri/src/state.rs` | 新增广播通道 | #3 |
| `src-tauri/src/remote/server.rs` | 新增广播收发 | #3 |
| `src-tauri/src/lib.rs` | 处理新事件 | #3 |
| `src-tauri/src/commands/pane.rs` | 触发广播 | #3 |
| `src-tauri/src/commands/terminal.rs` | 触发广播 | #3 |
| `src/remote/lib/wsRemote.ts` | 新增消息类型 | #3 |
| `src/remote/MainApp.svelte` | 处理推送消息 | #3 |
| `src/lib/stores/paneTree.ts` | 监听新事件 | #3 |
