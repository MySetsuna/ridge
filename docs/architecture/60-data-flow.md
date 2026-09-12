# 60 — 端到端数据通路

本文追踪每个 byte 从 PTY 子进程到最终渲染像素的完整路径。Foundation
收口后存在两条路径：
* **canonical**：kernel RTP1 WS ↔ shell `rtp1_kernel_client` ↔ renderer
* **legacy adapter**：kernel HTTP `bounded-seq-v1` ↔ shell `KernelPtyReader` ↔ renderer

## 路径 A：RTP1 WS（canonical）

```
PTY 子进程 stdout (portable_pty bytes)
        │ chunk
        ▼
[1] kernel: PtyBridge reader_thread
        │ mpsc::Sender<Vec<u8>>
        ▼
[2] kernel: spawn task（按 spawn_command_for_with_env 启动）
        │ bytes.recv().await
        ├─► [2a] screen.lock().feed(&bytes)        // ridge_term::Terminal 同步
        ├─► [2b] retained.extend_from_slice         // scrollback (cap 1 MiB)
        └─► [2c] hub.publish(&bytes)                // → PtyOutputHub
                │ bytes
                ▼
[3] kernel: PtyOutputHub（环形 256 KiB / 256 帧）
        │ notify_waiters()
        ├─► consumer A (HTTP lease)
        └─► consumer B (RTP1 WS output pump)
                │
                ▼
[4] kernel: rtp1_ws::run_output_pump
        │ lease.next(50ms, 64)
        ▼ frames
[5] kernel: Rtp1Session::build_output_frames("term", &frames)
        │ ≤ 24 KiB / chunk; total ≤ 64 KiB / frame
        ▼ RTP1 frames (Vec<rtp1::Frame>)
[6] kernel: mpsc::Sender<Frame>  →  tx
        ▼
[7] shell: rtp1_ws::drive (read loop)
        │ ws.send(Message::Binary(rtp1::encode(&frame)))
        ▼ WebSocket binary
[8] shell: rtp1_kernel_client::Rtp1KernelClient read loop
        │ mpsc::Receiver<OutputFrame>
        ▼ OutputFrame (parsed)
[9] shell: engine::kernel_pty → Tauri Channel
        ▼
[10] shell: PaneParser.feed → PaneDeltaMailbox
        ▼
[11] Tauri Channel → JS take_pane_delta_frame
        ▼
[12] JS: WASM Terminal.applyDelta
        ▼
[13] TerminalManager.render (RAF → WebGPU surface)
        ▼
[14] GPU present → pixels on screen
```

## 路径 B：HTTP long-poll（legacy adapter）

```
PTY 子进程 stdout
        │
        ▼
[1..3] 同 RTP1 路径
        │
        ▼
[4] shell: KernelPtyReader (HTTP long-poll GET /v1/domain/ptys/:id/output/:lease)
        │ 超时 ~50ms，命中即取 ≤ 64 帧
        ▼ bytes
[5] shell: PaneParser.feed → PaneDeltaMailbox
        ▼
[6..13] 同 RTP1 路径
```

## 输入路径（C→S：controller → PTY 子进程）

```
controller (browser / mobile / CLI)
   │
   ▼ RTP1 InputFrame { terminal_id, controller_id, input_seq, data_b64 }
   │
   ▼ kernel: rtp1_ws::handle_input
        │ controller_id alignment check (active_controller)
        ▼
   ▼ kernel: PtyRegistry::write_with_controller(pty_id, controller_id, &data)
        │ 检查 attached_controllers[pty_id].contains(controller_id)
        │ 否则返 PtyInputError::ControllerIdUnknown
        ▼
   ▼ kernel: PtyBridge.write_input(&data)
        │ portable_pty::MasterPty.writer.write_all + flush
        ▼
   PTY 子进程 stdin
```

控制权流转：
1. **legacy HTTP**：`/v1/domain/ptys/:id/write` body 含可选
   `controller_id`；缺省时 kernel 自动注册 `legacy-http:<pty_id>` 合成
   id，让 shell 的 `KernelPtyWriter` 在迁移窗口内继续工作。
2. **canonical RTP1**：WS adapter 在 `attach` / `detach` 时调
   `PtyRegistry::attach_controller` / `detach_controller`。

## session_event 派发（PTY exit）

```
PTY 子进程退出 → reader task 退出循环
        │
        ▼
[1] kernel: reader task 收尾
        ├─► hub.close()
        ├─► lifecycle[id] = Exited
        └─► exit_subs[id].send(PtyExitNotification)
                │
                ▼ broadcast::Receiver
[2] kernel: rtp1_ws::run_output_pump
        │ select! { exit = exit_recv.recv() => ... }
        ▼
[3] kernel: tx.send(SessionEvent{event:"exited", code, terminal_id})
        │
        ▼
[4] shell: rtp1_kernel_client read loop
        │ 收到 session_event
        ▼ state = Closing
[5] shell: 与 server detach_ack 配合收尾
```

## Service 边界

* [1]-[3] **kernel 进程内部**（in-process mpsc / broadcast）。
* [4]-[6] **kernel 进程 ↔ 消费者**：HTTP long-poll 或 RTP1 WS。
* [7]-[8] **kernel WS frame ↔ shell RTP1 client**（WebSocket transport）。
* [9]-[13] **shell 进程内部**（Tauri Channel / mailbox）。
* [14] **GPU present**（浏览器 / WebView）。

## 失败 / 重试语义

| 失败点 | 恢复路径 |
|---|---|
| PTY 子进程崩溃 | reader task 退出 → lifecycle=Exited → session_event{event:"exited"} |
| HTTP long-poll 超时 | 50ms 后 next poll；PTY idle 时不会消耗 CPU |
| RTP1 WS 断 | kernel side 立即通知 client；client 重连自动 attach + since_output_seq |
| Resync 触发 | 服务端先发 desync{reason}; client 自动 replay → snapshot |
| 客户端输入未注册 controller_id | kernel 返 error{code:"controller_id_unknown"} |
| 输入超 realtime cap | kernel 返 error{code:"input_too_large"} |

## 端到端测试

* `packages/ridge-cli/tests/rtp1_kernel_e2e.rs::rtp1_ws_full_lifecycle`：
  开真实 kernel 子进程 → WS attach → 全 message flow 验证 → detach。
* `packages/ridge-kernel/tests/terminal_live.rs`：16 个 live OS PTY 场景
  （UTF-8/ANSI/resize storm/emoji/alternate screen/...）。

## 详尽规范

* Kernel：`docs/architecture/10-kernel.md`
* RTP1 wire：`docs/architecture/20-rtp1-wire.md`
* Lifecycle：`docs/architecture/30-remote-lifecycle.md`
