# Remote Raw Byte 转发架构重构 — 审核提示词

## 1. 问题背景

Ridge Remote 服务在桌面浏览器连接后出现以下症状：
- 终端全屏空白，仅光标闪烁
- Remote 服务进程崩溃
- 整机系统 Crash（Windows）

根因排查发现三层缺陷：

### 1.1 架构缺陷：Per-Sub PaneParser 内存放大

每个远程 subscriber 创建独立 `PaneParser` 实例，内部持有 `snapshot: Vec<Vec<DeltaCell>>`（`rows × cols` 单元格）。500×500 = 25 万单元格，每单元格 ~45 bytes → **~11MB/parser**。N 个 remote sub = N+1 倍 parser 内存，且 resize 无上限（仅 `max(1)` 无 `min(500)`），可导致 OOM。

### 1.2 通道积压 + 丢帧 + 状态脱节

mpsc 通道 `try_send` 满时静默丢弃 delta，但 `PaneParser::feed_and_diff` 已更新 snapshot。客户端 wasm kernel 收到不连续 delta → `applyDeltaFrame` 应用到不一致状态 → 渲染空白/错误。

### 1.3 JS 内存泄漏

- `terminalController.ts`: document 事件监听器（`visibilitychange`、`loadingdone`）未在 `destroy()` 移除，闭包捕获 `this` 导致整个 controller 实例泄漏
- `wsRemote.ts`: `paneOutputs` Map 无界增长；`_pendingRequests` 未在 `disconnect()` 排空；`onmessage`/`onerror`/`onopen` 闭包未 nullify

---

## 2. 架构方案：单 Parser + Raw Byte 转发

### 2.1 核心思路

**删除全部 per-sub PaneParser**，remote 客户端服务端只转发原始 PTY 字节（不做解析、不编码 delta、不维护 parser 状态）。客户端 wasm kernel 自行调用 `feed()` 解析。

### 2.2 数据流对比

```
旧 (per-sub delta):
  PTY → lib.rs → for each sub: PaneParser.parse() → encode delta → mpsc → WS
      问题: N 倍内存, N 倍 CPU, try_send 丢帧 → 状态脱节

新 (raw byte broadcast):
  PTY → lib.rs → Arc<Vec<u8>> clone once → mpsc(512) → WS → client kernel.feed()
      优势: 0 parser, 0 delta 编码, 客户端自解析 = 无状态脱节
```

### 2.3 关键设计决策

| 决策 | 理由 |
|------|------|
| 使用 `Arc<Vec<u8>>` 而非 per-sub clone | 所有 sub 共享同一份字节，N 个 sub 只分配一次 |
| 客户端用 `kernel.feed()` 而非 `kernel.applyDeltaFrame()` | wasm vte parser 已生产验证（桌面端 wasm-mode 用同一路径） |
| Metadata (title/cwd/bell) 走独立 WS Text JSON 消息 | 从 binary path 解耦，避免在 raw bytes 中混入结构化数据 |
| mpsc 通道容量 512（原 128+256 合并） | 单通道统一管理，减少积压点 |
| resize 加 `min(500)` 上限 | 防御移动端异常大 viewport |

---

## 3. 变更清单

### 3.1 Rust: `src-tauri/src/types.rs`

**操作**: 删除 `PtyOutputEvent`、`PtyDeltaEvent`、`RawPtyEvent` 三个结构体，新增 `RemotePtyEvent` 枚举。

```rust
pub enum RemotePtyEvent {
    RawBytes { workspace_id, pane_id, bytes: Arc<Vec<u8>> },
    Metadata  { workspace_id, pane_id, title: Option<String>, cwd: Option<String> },
    Bell      { workspace_id, pane_id },
    PtyResized { workspace_id, pane_id, rows: u16, cols: u16 },
}
```

**审核要点**:
- `bytes: Arc<Vec<u8>>` 避免了 per-sub clone，确保 N 个 sub 共享一份分配
- `Metadata/Bell/PtyResized` 覆盖了此前嵌在 DeltaFrame 中的 metadata

---

### 3.2 Rust: `src-tauri/src/state.rs`

**操作**: 
1. 删除 `use crate::engine::parser::PaneParser`（不再需要）
2. `RemotePaneSub` 结构体：

```rust
// 旧（6 字段）
pub struct RemotePaneSub {
    pub id: u64,
    pub output_tx: mpsc::Sender<PtyOutputEvent>,   // 删除
    pub delta_tx: mpsc::Sender<PtyDeltaEvent>,       // 删除
    pub parser: Option<Arc<Mutex<PaneParser>>>,       // 删除 — 核心内存来源
    pub rows: u16,
    pub cols: u16,
}

// 新（3 字段）
pub struct RemotePaneSub {
    pub id: u64,
    pub raw_tx: mpsc::Sender<RemotePtyEvent>,         // 统一通道
    pub rows: u16,
    pub cols: u16,
}
```

3. `resize_remote_parser` 替换为 `update_remote_sub_dims`（仅更新 rows/cols，不操作 parser）

**审核要点**:
- `parser: Option<Arc<Mutex<PaneParser>>>` 的删除是否正确？（确认 parser 不再被任何代码路径使用）
- `resize_remote_parser` 的所有调用方是否已替换？

---

### 3.3 Rust: `src-tauri/src/lib.rs` — 核心变更

**操作**: 替换 PTY 输出分发逻辑的 remote fan-out 部分

```rust
// 旧 (lines ~180-298): 双通道模型
//   Path A: for sub in remote_subs → sub.output_tx.try_send(data.clone())  // text JSON
//   Path B: for sub in remote_subs → sub.parser.feed_and_diff() → encode → sub.delta_tx.try_send()
//   问题: delta_tx try_send 失败时 parser 已前进 → 状态脱节

// 新:
if app_state.remote_enabled.load(Ordering::Relaxed) {
    let reg = app_state.pty_pane_registry.read();
    if let Some(entry) = reg.get(&(workspace_id, pane_id)) {
        if !entry.remote_subs.is_empty() {
            let shared = Arc::new(data.as_bytes().to_vec());  // ← clone ONCE
            for sub in &entry.remote_subs {
                if sub.raw_tx.try_send(RemotePtyEvent::RawBytes {
                    workspace_id, pane_id,
                    bytes: Arc::clone(&shared),  // ← cheap ref-count clone
                }).is_err() {
                    tracing::warn!(target: "ridge::remote", sub = sub.id,
                        "raw byte channel full; dropping frame");
                }
            }
        }
    }
}
```

**审核要点**:
- `Arc::clone(&shared)` 是 ref-count increment，无额外堆分配 — 正确 ✓
- 丢帧时记录 warning 日志（vs 旧代码的 `let _ =` 静默丢弃）— 改善 ✓
- 桌面端 delta 路径（`PaneParser::feed_and_diff` → `encode_frame` → `sender(bytes)`）**保留完整**，未改动
- `continue` 跳过的 text coalescer 也保留完整

---

### 3.4 Rust: `src-tauri/src/remote/server.rs` — 核心变更

**操作 1**: mpsc 通道合并
```rust
// 旧（2 个通道）
let (output_tx, mut output_rx) = mpsc::channel::<PtyOutputEvent>(128);
let (delta_tx, mut delta_rx) = mpsc::channel::<PtyDeltaEvent>(256);

// 新（1 个通道，容量 512 = 256+128+缓冲区）
let (raw_tx, mut raw_rx) = mpsc::channel::<RemotePtyEvent>(512);
```

**操作 2**: `subscribe-pane` 简化（80 行 → 40 行）

删除的代码：
- 创建 `PaneParser::new(sub_rows, sub_cols, 5000)` 
- `get_recent_scrollback_for` → `feed_and_diff` → `full_reframe_with_scrollback` → `encode_frame` → 发送 bootstrap delta frame

保留的代码：
- `current_pane.take()` + `unregister_remote_sub`（切换 pane 时清理）
- `delta_mode.store(true)` 确保桌面端 parser 进入 delta 模式
- `register_remote_sub` 注册（字段改为 `raw_tx`）
- 发送历史 scrollback raw bytes（不解析、不 diff、不 encode）

**操作 3**: `resize` 简化（40 行 → 10 行）

```rust
// 旧: resize_remote_parser → 重新获取 sub_parser → full_reframe_with_scrollback → encode → send binary frame
// 新: update_remote_sub_dims（仅更新 rows/cols 记录）

mobile_rows = rows.max(1).min(500);   // ← 新增上限，解决无界分配
mobile_cols = cols.max(1).min(500);
ctx.state.update_remote_sub_dims(active_ws_id, pane_id, sub_id, mobile_rows, mobile_cols);
```

**操作 4**: tokio::select! 循环简化（4 分支 → 3 分支）

删除 `output_rx.recv()` 和 `delta_rx.recv()` 两个分支，新增 `raw_rx.recv()` 统一分支，处理 4 种 `RemotePtyEvent` 变体：
- `RawBytes` → Binary WS 帧 `[16B UUID][raw bytes]`
- `Metadata` → Text JSON `{"type":"pty-meta","paneId":"...","title":...,"cwd":...}`
- `Bell` → Text JSON `{"type":"pty-bell","paneId":"..."}`
- `PtyResized` → Text JSON `{"type":"pty-resized","paneId":"...","rows":...,"cols":...}`

**操作 5**: 修复 welcome 失败时 client registry 泄露
```rust
// 旧：return 时未调用 remote_client_registry.unregister(client_id)
// 新：return 前执行 unregister
ctx.state.remote_client_registry.unregister(client_id);
```

**操作 6**: `apply_pane_resize` 远程通知改用 `PtyResized` 事件
```rust
// 旧: sub.delta_tx.try_send(PtyDeltaEvent { bytes: bytes.clone() })
// 新: sub.raw_tx.try_send(RemotePtyEvent::PtyResized { rows, cols })
```

**审核要点**:
- `subscribe-pane` 发送 raw scrollback → 客户端 kernel.feed() 解析历史。审核确认 scrollback 是 raw PTY bytes（UTF-8 安全，不是 postcard delta） ✓
- `update_remote_sub_dims` 只记录尺寸 → 客户端自行调用 `kernel.resize()` ✓
- welcome 失败时的 `unregister` 调用是否在 `client_id` 有效时执行 ✓

---

### 3.5 JS: `src/remote/lib/terminalController.ts`

**操作 1**: 修复 document 事件监听器泄露

```typescript
// 新增字段存储 handler 引用
private _visibilityHandler: (() => void) | null = null;
private _fontHandler: (() => void) | null = null;

// setup 时存储引用
this._visibilityHandler = () => { ... };
document.addEventListener('visibilitychange', this._visibilityHandler);

// destroy() 中移除
if (this._visibilityHandler) {
    document.removeEventListener('visibilitychange', this._visibilityHandler);
    this._visibilityHandler = null;
}
if (this._fontHandler) {
    document.fonts?.removeEventListener('loadingdone', this._fontHandler);
    this._fontHandler = null;
}
```

**操作 2**: 修复 setTimeout/rAF 计时器类型混淆

```typescript
// 新增字段
private sleepTimerId: ReturnType<typeof setTimeout> | null = null;

// scheduleNextFrame 中 setTimeout 存入 sleepTimerId（不再覆盖 rafId）
this.sleepTimerId = setTimeout(() => { ... }, msUntilBlink - 8);

// destroy 中分别清理
if (this.rafId !== null) cancelAnimationFrame(this.rafId);
if (this.sleepTimerId !== null) clearTimeout(this.sleepTimerId);   // ← 新增
```

**操作 3**: 新增公开方法

```typescript
setTitle(title: string) { if (this.onTitle) this.onTitle(title); }
kernelResize(rows: number, cols: number) {
    this.rows = rows; this.cols = cols;
    this.kernel.resize(rows, cols); this.needsRender = true;
}
```

---

### 3.6 JS: `src/remote/lib/wsRemote.ts`

**操作 1**: 新增监听器 API

```typescript
export type RawByteListener = (paneId: string, data: Uint8Array) => void;
export type MetaListener = (paneId: string, title: string|null, cwd: string|null) => void;
export type BellListener = (paneId: string) => void;
export type PtyResizeListener = (paneId: string, rows: number, cols: number) => void;

class RemoteConnection {
    onRawBytes(fn: RawByteListener): () => void;
    onMetadata(fn: MetaListener): () => void;
    onBell(fn: BellListener): () => void;
    onPtyResize(fn: PtyResizeListener): () => void;
}
```

**操作 2**: binary 消息改为路由到 `rawByteListeners`

```typescript
// 旧: buf.slice(16) → binaryDeltaListeners
// 新: buf.subarray(16) → rawByteListeners  // subarray = zero-copy view
```

**操作 3**: paneOutputs 加 5000 行上限

```typescript
const MAX_PANE_OUTPUT_LINES = 5000;
if (existing.length > MAX_PANE_OUTPUT_LINES) {
    existing.splice(0, existing.length - MAX_PANE_OUTPUT_LINES);
}
```

**操作 4**: disconnect() 全面清理

```typescript
disconnect() {
    if (this.ws) {
        this.ws.onopen = null; this.ws.onerror = null;
        this.ws.onmessage = null; this.ws.onclose = null;  // 全部 nullify
        this.ws.close(); this.ws = null;
    }
    this.setState('disconnected'); this.paneOutputs.clear();
    for (const [, pending] of this._pendingRequests) {
        pending.reject(new Error('disconnected'));  // 排空 pending promises
    }
    this._pendingRequests.clear();
}
```

---

### 3.7 JS: `src/remote/MainApp.svelte`

连线新 API：

```typescript
ws.onRawBytes((paneId, data) => {
    if (paneId === activePaneId) canvasRef?.feedUtf8(data);  // raw bytes → kernel.feed()
});
ws.onMetadata((paneId, title) => {
    if (title != null && paneId === activePaneId) canvasRef?.setTitle(title);
});
ws.onPtyResize((paneId, rows, cols) => {
    if (paneId === activePaneId) canvasRef?.resizeKernel(rows, cols);
});
```

---

## 4. 审核检查清单

### 4.1 架构层面
- [ ] per-sub PaneParser 的所有创建路径是否已全部删除？（server.rs, state.rs）
- [ ] 旧 mpsc 通道（output_tx, delta_tx）的所有引用是否已清理？
- [ ] 桌面端 delta 路径是否完整保留且不受影响？
- [ ] `Arc<Vec<u8>>` 的使用是否正确？（只分配一次，多 sub 共享引用计数）

### 4.2 资源管理
- [ ] WS disconnect 时 RemotePaneSub 的清理是否完整？（`unregister_remote_sub`）
- [ ] 新建的 `raw_tx` mpsc sender 在 disconnect 时是否随 `RemotePaneSub` Drop？
- [ ] `subscribe-pane` 发送的历史 scrollback 是否正确？（raw bytes, 64KiB, UTF-8 安全）

### 4.3 客户端兼容性
- [ ] 客户端 wasm kernel 的 `feed()` 方法能否正确处理从 WS 接收的 raw bytes？
- [ ] `onRawBytes` 中 `buf.subarray(16)` 的 zero-copy 行为是否正确？（不复制，只 view）
- [ ] metadata 消息（pty-meta, pty-bell, pty-resized）格式与服务端一致？

### 4.4 边界条件
- [ ] resize 上限 `rows.min(500)` 和 `cols.min(500)` 是否足够？
- [ ] mpsc 通道容量 512 在 `Arc<Vec<u8>>` 共享模型下是否足够？
- [ ] welcome 发送失败时 client registry 清理是否正确？

---

## 5. 验证方式

```bash
# Rust 编译
cd src-tauri && cargo check

# Rust 测试
cd src-tauri && cargo test

# TypeScript 类型检查
pnpm run check

# 功能测试（需 Tauri 环境）
# 1. 启动 Ridge Desktop
# 2. 在设置中启用 Remote Control
# 3. 用手机/桌面浏览器连接
# 4. 在终端中输入命令，确认输出正常显示
# 5. 断开重连，确认历史滚动回溯恢复正确
# 6. 切换 pane，确认切换正常
```

---

## 6. 潜在风险

| 风险 | 缓解 |
|------|------|
| 原始字节带宽增大 3-5x | 局域网可忽略；后续可加 WS permessage-deflate |
| 移动端 wasm feed() 解析 CPU 开销 | 已有 `feedChunked` 时间预算机制（4ms/batch），桌面端已验证 |
| WS Binary 帧格式变更（delta→raw）需客户端同步更新 | 客户端 wsRemote.ts 已同步更新 |
| `PtyOutputEvent`/`PtyDeltaEvent`/`RawPtyEvent` 被引用 | 已全量搜索确认只在 types.rs 中定义，无外部引用 |
