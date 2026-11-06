# 50 — Desktop 端（Tauri + WebGPU）

> 包：`src-tauri/`
> 顶层二进制：`ridge.exe`（Tauri 桌面端）
> 渲染：`TerminalManager` 单 canvas + WebGPU surface

Desktop 端是 Ridge 架构的**渲染入口**。它不拥有 PTY 子进程，
只通过 kernel HTTP + RTP1 WS 通道消费字节流；本地 kernel 进程
（`ridge-kernel.exe`）由 desktop 启动并作为后台进程存活。

## 进程模型

```
┌──────────────────────────────────────────────────────────┐
│ ridge.exe (Tauri)                                        │
│   ┌────────────────────────────────────────────────────┐ │
│   │ WebView (JS)                                       │ │
│   │   terminal/manager.ts → WebGPU surface              │ │
│   │   WASM kernel (ridge_term) → grid + scrollback      │ │
│   └──────────┬───────────────────┬────────────────────┘ │
│              │ JSON-RPC          │ WS                  │
│   ┌──────────▼────────────┐ ┌─────▼──────────────────┐ │
│   │ commands/terminal.rs  │ │ rtp1_kernel_client     │ │
│   │ engine/parser.rs      │ │ (via ridge-cli module) │ │
│   │ engine/pty.rs         │ │                        │ │
│   └──────────┬────────────┘ └─────────┬──────────────┘ │
└──────────────┼─────────────────────────┼────────────────┘
               │ HTTP (legacy) / RTP1 WS  │
       ┌───────▼──────────────────────────▼──────┐
       │ ridge-kernel.exe (control plane + PTY)   │
       │ /v1/domain/ptys + /v1/rtp1 WS           │
       └────────────────────────────────────────┘
```

## 模块（`src-tauri/src/`）

| 模块 | 角色 |
|---|---|
| `commands/terminal.rs` | Tauri command（create_pane / write / resize / close） |
| `engine/pty.rs` | PTY reader 引擎（kernel-backed HTTP adapter） |
| `engine/parser.rs` | VTE 解析路径（用于本地备援 / 移动端镜像） |
| `engine/kernel_pty.rs` | kernel-backed HTTP 长轮询 |
| `state.rs` | `Workspace` / `PtyHandle` / `PaneTree` |
| `hosts/outbound.rs` | 远程 host outbound client（旧私协议） |
| `hosts/reconnect_supervisor.rs` | 重连 / 多 host 隔离 |
| `hosts/lan_transport.rs` | LAN remote 控制面 + 自签 TLS |
| `remote_host_impl.rs` | `RemoteHostImpl`（controller 视图） |
| `utils/pty_log.rs` | 给 PTY 镜像写日志 |

## PtyHandle 状态

```rust
struct PtyHandle {
    master: ...           // legacy：直接 master fd（已被 deprecated）
    writer: ...           // legacy：直接 writer
    _child: ...           // legacy：本地 child handle
    native_ref: Option<NativePtyRef>  // legacy 备份
    kernel_ref: Option<KernelPtyRef>,  // ← kernel-backed（authoritative）
    parser, delta_mode, ...
}
```

`native_ref` / `_child` 字段标 deprecated；新代码禁止使用。`kernel_ref`
是唯一的 authoritative 句柄。

## Tauri command → kernel 路径

```rust
// Tauri command `terminal.write`
Tauri → spawn_blocking(create_pane_inner_*)
     → ensure_pane_pty_workspace_with_initial_size
     → try_install_kernel_pty
     → install_shell_kernel_pty
     → attach_or_spawn_kernel_pty        // HTTP POST /v1/domain/ptys
     → PtyHandle { kernel_ref: Some(...) }
     → spawn_pty_reader (HTTP long-poll per read)
```

## Kernel ↔ renderer 数据通路

```
PTY 子进程 stdout
  → kernel PtyOutputHub.publish → notify
  → shell KernelPtyReader (HTTP long-poll GET /v1/domain/ptys/:id/output/:lease)
  → shell PaneParser → PaneDeltaMailbox
  → Tauri Channel → JS take_pane_delta_frame
  → WASM Terminal.applyDelta
  → TerminalManager.render (WebGPU surface)
```

旧路径是 HTTP long-poll；RTP1 WS 路径通过 `rtp1_kernel_client` 提供，
但 desktop Tauri 集成的完整迁移尚未收口（HTTP 仍是默认）。

## WebGPU 渲染

* `TerminalManager` 单 canvas + WebGPU surface
* `present_fast` 优化（`localStorage.RIDGE_PRESENT_FAST`）
* WASM grid kernel 来自 `packages/ridge-term`（与 kernel 共享 native vte core）

## 测试

* `src-tauri/src/engine/parser.rs::tests::*` — VTE 解析
* `src-tauri/src/utils/pty_log.rs::tests::*` — 日志写入
* 端到端（`pnpm e2e:shell` / `pnpm e2e:perf`）— **gated by release build + headed runner**，
  Foundation 范围内无法跑。

## Live e2e 暂未覆盖

`pnpm e2e:shell` / `pnpm e2e:perf` 需要 release build + headed Windows
runner，Foundation 范围内不能跑。kernel 侧的 live WS e2e
（`packages/ridge-cli/tests/rtp1_kernel_e2e.rs`）作为权威替代。

## 详尽规范

* 桌面端 spec：`specs/L2-TERM-001.md`（terminal rendering contract）
* 端到端 ：`docs/architecture/60-data-flow.md`
* Foundation 终报告：`docs/architecture/RIDGE-RUNTIME-FOUNDATION-FINAL.md`
