---
id: L2-TERM-001
level: L2
title: Terminal Rendering Contract
status: APPROVED
origin: authored
migration_state: PROPOSED
depends_on: [L1-PROJECT-001]
---

## §1 范围

覆盖自 PTY 子进程字节流出，至前端 grid 像素呈现的整条数据通路。
不含：MCP / Git / File tree / Search / Teammate 协议。

## §2 现状事实（按代码）

### §2.1 数据通路六层（当前实际）

| 层 | 模块 | authoritative owner | 输入 | 输出 |
|---|---|---|---|---|
| L1 PTY | `portable_pty::native_pty_system` 起的子进程 + master fd | kernel PtyRegistry（PTY 进程）；shell `PtyHandle.master`（镜像） | — | bytes（u8 流） |
| L2 reader | kernel: `crates/pty.rs` 内部 reader 线程；shell: `engine/pty.rs` `spawn_pty_reader` | 双源（见 §6） | bytes | bytes 切片 |
| L3 output hub | kernel `PtyOutputHub`（256 KiB / 256 frames 环形，per-PTY）；shell `pty_scrollback` `PaneScrollback`（block 64 KiB × N，8 MiB cap） | kernel authoritative 输出缓冲；shell scrollback 镜像 | bytes | `PtyOutputFrame { seq, data }`；`ScrollbackChunk { bytes, start_seq, end_seq, at_oldest, head_seq }` |
| L4 parser | shell `engine/parser.rs::PaneParser`（桌面/云均走同一份）；kernel 用 `portable_pty` + `ridge_term::Terminal`（用于 foreign pane host 渲染与 capture） | shell parser 是 v1 唯一对外接口；kernel 用 `Terminal` 做"复活式"重渲 | bytes | grid 状态 + `PendingEvents`（Title/Cwd/Bell/IconName）；每条 byte 也喂 PtyHandle.parser |
| L5 delta/raw | shell `engine/parser.rs` 解析期间产出 `DeltaFrame { version, pane_seq, deltas }`；kernel 暂无同形 delta（仅 PtyOutputFrame） | shell 端 delta（postcard 编码）；kernel 端 raw（base64 in JSON） | parser grid mut | `DeltaFrame`（postcard bytes）；或 `PtyOutputFrame { seq, data_b64 }` |
| L6 transport | 桌面: `PaneDeltaSender` Tauri Channel → `emit_pane_delta` → JS `take_pane_delta_frame`；LAN remote: WS `RemotePtyEvent::RawBytes`/`SemanticDelta`；Cloud: `subscribe_pane_terminal_v2` + `subscribe_pane_raw`；kernel HTTP: `/v1/domain/ptys/{id}/output/{lease}` long-poll | 双源（per-connection 投递） | delta / raw bytes / snapshot | 进程内 Channel、WS binary frame、HTTP JSON |
| L7 frontend model | `ridge_term::Terminal` WASM（grid + cursor + scrollback + selection + search）；`PaneParser` 的 desktop mirror | WASM kernel；desktop parser mirror | delta / snapshot | grid cell |
| L8 renderer | `TerminalManager`（WebGPU surface，单 canvas 多 pane）；`RenderHandle::render(kernel)` | 单 GPU surface；JS `terminal/manager.ts` RAF loop | grid cell | GPU 帧 |

### §2.2 四种数据形态（按代码）

| 形态 | 定义位置 | 序列字段 | 大小上限 | 重传语义 |
|---|---|---|---|---|
| **RawByteStream** | kernel `PtyOutputFrame { seq, data }`；shell `pty-output-{ws}-{pane}` 事件 payload `{data: str}` | seq 严格单调 | kernel OUTPUT_REPLAY_CAP_BYTES=256 KiB / 256 frames；shell `SCROLLBACK_MAX_BYTES=8 MiB` | lease cursor 落后 → `Lagged{requested_seq, oldest_seq, latest_seq}` 必 resync |
| **TerminalDelta** | `ridge_term::term::delta::{DeltaFrame, GridDelta, DeltaCell, DeltaLine}`；shell mailbox 编码 postcard | pane_seq 严格单调；out-of-order 帧在 `PaneDeltaMailbox::push` 拒绝（NeedsResync） | 帧内 `MAX_DELTA_MAILBOX_DELTAS=8192` deltas；超限 → NeedsResync | 顺序合并 `merge_ordered_frames`；乱序则强制 reframe |
| **TerminalSnapshot** | `ridge_term::remote_v2::TerminalSnapshot`（revision、ScreenSnapshot、cursor、modes、title、cwd、scrollback）；kernel 无对应 | revision 单调 | scrollback 行数由构造时固定 | 替换前端整个 mirror；不必与历史合并 |
| **RenderFrame** | WASM `Terminal::cells()` + `RenderHandle::render(kernel)` → GPU `wgpu::Surface` | 不序列号；按 rAF 节拍 | 全 grid；脏行优化 `present_fast`（`localStorage.RIDGE_PRESENT_FAST`） | 仅前端语义，无 wire 字段 |

### §2.3 Buffer 与背压

- **shell `PaneDeltaMailbox`**（state.rs）：`pending: Option<DeltaFrame>` 单槽合并；`wake_armed: bool`；`last_sequence`；超出 `MAX_DELTA_MAILBOX_DELTAS` → NeedsResync（请求重发整帧）。
- **kernel `PtyOutputHub`**：环形 buffer；`OUTPUT_REPLAY_CAP_BYTES=256 KiB` & `OUTPUT_REPLAY_CAP_FRAMES=256` 双 cap；超限按先进先出丢头。
- **shell `pty_scrollback`**：block 64 KiB，块边界 UTF-8 safe（`is_utf8_char_boundary` 强制对齐），总 8 MiB 上限（每 pane）。
- **transport 背压**：remote WS `RemotePaneSub.raw_tx` 是 `tokio::sync::mpsc`；`try_send` 满则置 `desync` flag，下一帧触发 RIS + 全帧 scrollback resync（lib.rs `handle_pty_output` 分支）。
- **cloud pane**：refcount `cloud_pane_raw_subs` 仅在所有 controller 退订后才卸订阅；`cloud_pane_terminal_subs` 是 activation-scoped（多 controller 各持 envelope）。

### §2.4 顺序与生命周期

- **PTY 进程**：kernel `PtyRegistry.begin_destroy` → `bridge.destroy()` → `finish_destroy`；shell `pty_generation` 单调递增防 teardown 与新 spawn 竞态（state.rs `Workspace.pty_generation`）。
- **lease**：`attach_output(after_seq)` → `next(timeout, max_frames)` long-poll → `resync` 或 `detach`；`Detached|Closing|Closed|TimedOut|InvalidBatchSize` 5 错误码。
- **delta mailbox**：`register_pane_delta_channel` → 替换式注册（idempotent）→ `enqueue_pane_delta_frame` → 合并 → `take_pane_delta_frame`（一个 0 字节 wake 触发 JS 端取）。
- **scrollback 取回**：`tail(max_bytes)` / `before(before_seq, max_bytes)` / `since(since_seq, max_bytes)` 三种 cursor（同 `pty_scrollback.rs`）。
- **错误处理**：长轮询超时 = 空事件（200 / timeout）；lease 关闭 = `bad_request`；PTY 死 = `PtyOutputLeaseError::Closing/Closed`。

### §2.5 关键并发保护（代码已落地）

- `attach_transaction: Mutex<()>`：同一 host session 不允许并发 attach。
- `WorkspaceWindowClaims.claim` 多线程 barrier 测试确认 Exactly-One 拥有者。
- `pty_input_lanes: (ws, pane, source) → Mutex<PtyInputSequenceState>`：同源序列化、去重。
- `PaneDeltaMailbox` 单槽：合并前拒绝乱序 seq。
- `global_event` mpsc 容量 1024（`event_tx`），防 cat 大文件 backpressure。

## §3 契约（本 spec 的目标态，仅描述）

> 本 spec **不**要求补齐功能；下列条目均描述"当前代码已实现的契约"或"已留口待迁移"。

### §3.1 Authoritative owner 表

> 限定本表只描述"谁拥有字节序、谁拥有语义、谁拥有呈现"这三件事；不锁定"哪个 crate 永久实现 parser"。parser 可在 kernel / shell / WASM 任一处实现，只要满足下列 invariant 即可。

| 资源 | 当前 owner | 期望 invariant | 迁移方向 |
|---|---|---|---|
| **PTY 字节序（bytes ordering）** | 双源（kernel PtyOutputHub seq / shell `pty_pane_registry` seq） | **kernel authoritative**：PTY 子进程字节流的全局序由 kernel 唯一产生；lease cursor 必须能唯一定位 kernel seq | shell 端改用 lease 读取（消费 `PtyOutputFrame.seq`）；删除 `pty_scrollback` 镜像 |
| **PTY 进程 lifecycle** | 双源（kernel PtyRegistry + shell `PtyHandle._child`） | **kernel authoritative**：spawn/destroy/begin_destroy/finish_destroy 由 kernel 调度 | shell `PtyHandle` → lease-backed（仅持 `lease_id + cursor`），`native_ref/_child` 字段标 deprecated |
| **PTY 进程 lifecycle 事件** | 双源（kernel `bridge.destroy` / shell `pty_generation`） | **kernel authoritative**：close/resize/exit 事件由 kernel 单边发出 | close_pane 走 kernel API；shell 仅消费事件 |
| **semantic delta producer** | shell `engine::parser::PaneParser`（v1） | **允许向 kernel 收敛**：semantic delta 的生产者可以是 kernel 内 `Terminal` 实例，也可以是 shell parser；不变的是"每个 byte 必恰好进入一个 producer 一次、不重复解析" | 桌面 parser 改消费 `TerminalDelta`（已在 `ridge_term::remote_v2` 落地）；kernel 端 producer 由 SEPARATE 部分（`/v1/domain/ptys/{id}/delta`）按需提供 |
| **client render state** | WASM `Terminal`（JS 镜像） | **client presentation-local**：grid / cursor / scrollback / selection / search 均为 client 端 mirror；服务端的 `TerminalSnapshot` 必能完整重建 client mirror | 保留 WASM `Terminal`；不锁定其在哪个 host 进程实现 |
| **render pipeline** | `TerminalManager` 单 canvas + WebGPU surface | **client presentation-local**：rAF / GPU surface 属渲染层；与 producer/owner 解耦 | 不变 |
| **scrollback 取回** | shell `pty_scrollback` block store | **kernel authoritative replay**：client 通过 `replay`/`snapshot` 协议获取，不维护服务端之外的镜像 | shell 调用 `kernel::pty_output_poll` 或 envelope `replay_data`；删除 `PaneScrollback` |
| **recovery** | 局部（mailbox NeedsResync、remote desync） | **由 snapshot/replay protocol contract 定义**（见 SPEC-PROTO-001 §3） | 不在本 spec 定义；任何 owner 迁移不得绕过该契约 |

> parser 实现可分阶段：v1 仍由 shell `PaneParser` 解析；后续由 kernel `Terminal` producer 接管，但"byte 恰好进入一个 producer 一次"必须保留。
>
> 关键不变量（迁移期硬约束）：
> 1. PTY 字节流全局序 = kernel seq；任何 client 不可见"非 kernel 序"的字节流。
> 2. semantic delta 不可重放同一段 bytes 两次。
> 3. client mirror 可被任意 `TerminalSnapshot` 完全替换；不依赖 producer 端增量。

### §3.2 数据格式契约

- **RawByteStream**：`{ seq: u64, data: bytes }`；seq 严格单调；lease cursor 落后 oldest_seq → Lagged（must resync）。
- **TerminalDelta**：`{ version: u32, pane_seq: u64, deltas: Vec<GridDelta> }`；postcard 编码；乱序 seq 拒绝；超大帧 NeedsResync。
- **TerminalSnapshot**：`{ revision: u64, screen: ScreenSnapshot, cursor, modes, title, cwd, scrollback: Vec<WireLine> }`；替换式。
- **RenderFrame**：前端内部概念；不入 wire。

### §3.3 Buffer / 背压契约

- 任何 layer 满 → 必须给出"明确恢复路径"（resync / reframe / RIS），不允许静默丢。
- 序列号必须严格单调，丢包由 cursor + resync 重建。
- UTF-8 块边界：scrollback 冻结不允许切开 codepoint（已实现 `is_utf8_char_boundary`）。

### §3.4 责任分层（cross-spec 引用）

| 维度 | owner | spec |
|---|---|---|
| PTY 字节序、PTY 进程 lifecycle | kernel | 本 spec |
| wire envelope、attach/detach、input/output sequence identity、replay/snapshot/resync 协议 | 协议 | SPEC-PROTO-001 §3 |
| attach 状态机、reconnect 剧本、terminal/session 退出通知 | 协议 + 状态机 | SPEC-REMOTE-001 §3 |
| 测量指标、故障注入、baseline | 测量 | SPEC-PERF-001 §3 / §4 |

> 本 spec 范围内**不**对"哪个 host 进程实现 parser"作永久规定；只在 §3.1 给出当前 owner 与 invariant。任何 owner 迁移须满足上述 invariant 与 SPEC-PROTO-001 §3.5 / §3.7 的 recovery 契约。

## §4 Terminal 能力兼容矩阵（当前代码事实，不补实现）

> 标尺：`SUPPORTED` 完整实现；`PARTIAL` 已实现但有边界 bug；`PASSTHROUGH` 透传不过滤；`UNSUPPORTED` 未实现；`UNKNOWN` 找不到证据。

| 能力 | 状态 | 证据 |
|---|---|---|
| UTF-8 | SUPPORTED | scrollback 强制 codepoint 边界（state.rs） |
| CJK wide | SUPPORTED | `wcwidth.rs`；`DeltaCell.width` 含 0/1/2（continuation/half/full） |
| Emoji ZWJ cluster | SUPPORTED | `DeltaCell.cluster: Option<Box<str>>` 显式承载 extended grapheme cluster（delta.rs §emoji-cluster） |
| Skin-tone / VS16 | SUPPORTED | 同上（cluster 字段处理） |
| Combining marks | SUPPORTED | `term::cell` 处理组合字符 |
| Cursor | SUPPORTED | Cursor 变体：position / DECSCUSR / DECTCEM；`CursorShape` Block/Bar/Underline |
| CSI | SUPPORTED | parser.rs 解析 C0/C1；含 DEC private |
| OSC | PARTIAL | OSC 0/1/2（title）、OSC 7（cwd）、OSC 52（clipboard 状态）、OSC 133/633（prompt markers）；shell handler 可能漏边界 |
| Alt screen | SUPPORTED | `is_alt_screen()` 暴露；DECSET 1049 |
| 256 color | SUPPORTED | `Color` enum Indexed(0..=255)；`get_scm_status_fast` 等已使用 |
| True color | SUPPORTED | `Color` enum Rgb(u8,u8,u8)；SGR 38;2/48;2 |
| Mouse | SUPPORTED | `isMouseReporting/Button/Any/Sgr`；`encodeMouse`（lib.rs）；DECSET 1000/1002/1003/1006 |
| Bracketed paste | SUPPORTED | `encodePaste` + `isBracketedPaste`；CSI ?2004h |
| Resize | SUPPORTED | `Terminal::resize(rows, cols)`；kernel `domain_pty_resize` |
| Scrollback | SUPPORTED | `scrollback_lines` 构造；`scrollUp/Down/ToBottom`；user-scroll-lock |
| Clear | SUPPORTED | `clear_scrollback` API（kernel）；ED/DEC 解析（parser） |
| Title | SUPPORTED | OSC 0/1/2 → `PendingEvents::TitleChanged` |
| Hyperlink | SUPPORTED | `term::cell::HyperlinkSpan`；OSC 8 |
| Wide char fallback（缺字形） | UNKNOWN | WASM 字体 atlas 行为未排查 |
| DCS / sixel / iTerm image | UNKNOWN | parser 中有无专门 DEC 解析路径未确认 |
| Kitty graphics | UNSUPPORTED | 无 |
| DA1/DA2 设备属性查询 | PASSTHROUGH | PTY 直接给 shell（无主动回复） |

> 不补齐任何未支持能力。

## §5 端到端 Acceptance

1. **PTY→WASM 一致性**：同一段 `printf '\x1b[31m中文🎉\x1b[0m\n'` 在 desktop 与 headless host 渲染结果字符级一致（含 cluster 完整性）。
2. **顺序保证**：连续 10 次小写 PTY 写 + 强制 flush，UI 终端显示顺序与写入顺序一致；不存在"超前后倒置"。
3. **背压触发**：PTY 突发 1 MiB bytes，shell `PaneDeltaMailbox` 不越界、远端 WS 满则置 desync + RIS。
4. **lease resync**：consumer cursor 落后 → 收到 `Lagged` 并在 resync 后 grid 与 baseline 等价。
5. **snapshot 替换**：一次 `TerminalSnapshot` 应用后，前端 grid 与 snapshot `revision` 严格一致。
6. **错误边界**：PTY 子进程被 SIGKILL → `PtyOutputLeaseError::Closing/Closed` 在 ≤1 s 内上报；lease 自动 drop。

## §6 双源问题（仅描述，不迁移）

- shell `PtyHandle { master, writer, _child }` 与 kernel `PtyRegistry` 各持一份 PTY 进程句柄。
- `state.rs::PtyHandle.kernel_ref: Option<…>` 已留位（多数代码路径仍走 `native_ref`/`remote_ref`）。
- detach/close/reattach 状态机需双向同步（`pty_generation` 仅缓解竞争，并未消除双源）。
- 迁移边界：shell `terminal::create_pane` 改为调用 `kernel client::pty_create`，PtyHandle 改为 lease-backed（仅持 lease_id + cursor）；`native_ref/_child` 字段标 deprecated 但保留编译期可用；新代码禁止使用。
- 影响范围：`commands/terminal.rs`、`engine/pty.rs`、`state.rs`（PtyHandle 字段）、`hosts/mod.rs` 的 foreign attach、teammate `summon` 路径。
- 不本轮执行。
