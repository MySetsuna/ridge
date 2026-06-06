# rdg：交互式 TUI + LAN 化 + 公网控制端（Phase E 设计）

> 目标（用户原话）：把 `ridge-cli`（现可执行名已改为 `rdg`）改造成"没有 UI 的 ridge"——
> 能登录、能局域网控制、能公网控制，并且是**可交互的 TUI**。
>
> 本文是落地蓝图：按可独立交付、且每步保持 `cargo build` 绿色的顺序拆分。

## 0. 现状（基线，已核实）

`rdg`（`packages/ridge-cli`）今天能做：
- **云端登录**：设备码流（`device_flow.rs`）→ `~/.config/ridge/auth.json` 持久化 device JWT。
- **公网 host（应答方）**：`rdg remote --daemon` = 信令 WS(role=host) → WebRTC answerer → E2EE → TOTP 闸 → mux → JSON-RPC → PTY 桥（`daemon.rs`/`session.rs`/`rtc.rs`/`e2ee.rs`/`mux.rs`/`rpc.rs`/`pty.rs`）。
- **无头 tmux 引擎**：`rdg tmux`。

`rdg` 今天**不能**做（本设计要补齐）：
| 能力 | 状态 | 缺口 |
|---|---|---|
| LAN host（局域网被控端） | ❌ | 整套 LAN 服务器(axum HTTP/WS + TLS + mDNS + 会话/TOTP)只在 `src-tauri/src/remote/`，与 `AppState` 强耦合 |
| LAN controller（局域网控制端） | ❌ | 无客户端连接代码 |
| 公网 controller（发起方/offerer） | ❌ | `rdg` 永远是 answerer，无 offerer 路径 |
| 交互式 TUI | ❌ | 无任何 TUI；现仅命令式 daemon/打印 |

复用底座 `ridge-core`：`dispatch`（FS/search/theme 子集）、`CapabilitySet`/`REMOTE_ALLOWLIST`、`Ctx`/`EventSink`。
有状态命令（git/workspace/pane/写 FS）尚未迁入 `dispatch`，`rdg` 对这些返回 `METHOD_NOT_FOUND`。

## 1. 设计原则

1. **传输与渲染解耦**：TUI 只依赖一个 `RemoteSession`/`ControllerTransport` 抽象，不关心底层是 LAN WS 还是公网 WebRTC。
2. **渲染复用 `ridge-term` 内核**：`ridge-term` 的 VT 解析器 + grid（`term/` 模块，纯 Rust，无 wasm/gpu 依赖，`cargo check --lib` 在 host 上通过）直接喂 PTY 字节，再用 ratatui/crossterm 把 grid 画到终端。**不要在后方补空格——按 grid 单元自然输出，允许宽字符溢出**（与 §B.9 桌面渲染同语义）。
3. **TOTP 去重**：把 `src-tauri/src/remote/auth.rs::RemoteAuth` 与 `packages/ridge-cli/src/totp.rs::RemoteTotp` 合并到 `ridge-core`（用 CLI 的 `OsRng` 版本为准——desktop 当前用弱 PRNG），desktop + cli + 未来 LAN host 共用一份。
4. **LAN host 抽取**：把 `src-tauri/src/remote/` 中**与 Tauri 无关**的部分（tls/mdns/auth/会话注册/JSON-RPC 协商）下沉到一个新 crate `packages/ridge-remote-lan`（或并入 `ridge-core`），由桌面与 `rdg` 共用；与 `AppState` 耦合的多 workspace/pane 管理改为面向 trait。

## 2. 分步交付（每步独立、保持编译绿）

### 步骤 E1 — TOTP 下沉到 ridge-core（地基，低风险）
- 新增 `packages/ridge-core/src/totp.rs`：把 `ridge-cli/src/totp.rs::RemoteTotp` 整体搬入（`OsRng`、RFC6238 HMAC-SHA256、±1 窗口、`TOTP_PERIOD/DIGITS/SKEW` 常量、`base32_encode`/`hmac_sha256`）。导出 `pub use`。
- `ridge-cli/src/totp.rs` 改为 `pub use ridge_core::totp::*;`（或直接删除、改引用）。
- `src-tauri/src/remote/auth.rs::RemoteAuth` 改为内部持有 `ridge_core::totp` 的 secret/verify，删除其 `SimpleRng`。
- 验收：`cargo test -p ridge-core`（新增 TOTP 单测：已知 secret+时间→已知码）、`cargo check -p ridge-cli -p ridge`。

### 步骤 E2 — 交互式 TUI 外壳（先包住已有能力，立即可用）
- 依赖：`ratatui = "0.28"` + `crossterm = "0.28"`（成熟、纯 Rust、跨平台；Windows 控制台兼容）。
- 新增 `packages/ridge-cli/src/tui/`：
  - `app.rs`：TUI 状态机（登录态/设备列表/动作菜单）。
  - `ui.rs`：用 ratatui 画——顶部状态条（登录用户/在线设备）、主区菜单：
    `[1] 登录/授权  [2] 设备配对(--enable)  [3] 启动公网 host(--daemon)  [4] 连接控制…(E4)  [q] 退出`。
  - `event.rs`：crossterm 事件循环（键盘）。
- `main.rs`：**无子命令时进入 TUI**（`rdg` 直接起 TUI）；保留 `rdg remote/tmux` 子命令向后兼容。
- 复用现有 `device_flow`/`daemon`：菜单项直接调它们（daemon 在后台 task 跑，TUI 显示日志区）。
- 验收：`cargo check -p ridge-cli`；`rdg` 启动显示 TUI；菜单能触发登录/配对/daemon（host 能力本来就有）。

### 步骤 E3 — `ridge-term` grid 渲染适配器（TUI 内嵌终端视图）
- 新增 `packages/ridge-cli/src/tui/term_view.rs`：
  - 持有一个 `ridge_term::term::Terminal`（VT 解析器+grid），把 PTY/远端字节 `advance()` 进去。
  - 实现 `ratatui` 自定义 `Widget`：遍历 grid 行/单元，按 `Cell{ch,width,attr}` 转 ratatui `Span`，颜色取自 `attr_table`；宽字符占 `width` 列、**不补尾随空格**（空单元用空背景，宽字续半为跳过）。
- 注意：`ridge-term` 需以 `default-features = false`（不带 wasm/webgpu）作为 `rdg` 的依赖；其 `term/` 模块已是纯 Rust。
- 验收：单测——喂入一段含 CJK/ANSI 颜色的字节流，断言渲染出的 ratatui buffer 单元与 grid 一致、无尾随空格填充。

### 步骤 E4 — 公网 controller（offerer）+ TUI 接入
- 新增 `packages/ridge-cli/src/controller/`：镜像现有 host 路径，但作为 **offerer**：
  - 信令 WS `role=controller`（复用 `signaling.rs` 抽象）。
  - WebRTC offerer：创建 offer、收 answer、ICE（镜像 `rtc.rs` 的 `WebRtcHost` 写一个 `WebRtcController`）。
  - E2EE 发起方（`e2ee.rs` 已是对称的，复用）。
  - 提交 TOTP（CONTROL 0x12 `totp-verify`）。
  - 订阅 pane：发 JSON-RPC（`$/hello` 协商 → 订阅 → 收 0x10 PANE_RAW 字节喂给 E3 的 `term_view`）。
  - 输入：键盘 → JSON-RPC `write_to_pty` / resize。
- TUI 菜单项 `[4] 连接控制`：输入 `device-username` 或选已配对设备 → 建立 controller 会话 → 进入全屏终端视图（E3）。
- 验收：`rdg` 连接一个在线的桌面/`rdg --daemon` host，能看到远端终端并输入。需真机/双进程联调。

### 步骤 E5 — LAN host 抽取 + `rdg` LAN 化
- 新建 crate `packages/ridge-remote-lan`（Tauri-free），从 `src-tauri/src/remote/` 下沉：
  - `tls.rs`（rcgen 自签 CA+leaf）、`mdns.rs`（`_ridge._tcp.local.` 广播）、`auth`（会话 token store，复用 E1 的 TOTP）、`server.rs` 的 HTTP/WS 框架与 `$/hello` 协商。
  - 多 workspace/pane 管理改为 trait `LanHostState`（桌面用 `AppState` 实现；`rdg` 用单 workspace/单 pane 的 `HeadlessLanState` 实现）。
  - `src-tauri/src/remote/server.rs` 改为依赖该 crate（行为不变，回归用现有 e2e）。
- `rdg`：新增 `rdg lan --serve`（LAN host）与 TUI 菜单项；LAN controller 复用 E4 的 controller，传输换成 LAN WS 适配器。
- 验收：桌面 LAN 远控 e2e 全绿（回归）；`rdg lan --serve` 后手机/浏览器能在局域网连上。

## 3. 依赖与风险

- 新增 crates：`ratatui`、`crossterm`（E2/E3）；`ridge-term`（E3，default-features=false）。
- E5 是最大块（抽取 3000+ 行、面向 trait 重构），放最后；E1–E4 可先独立交付可见价值。
- WebRTC offerer（E4）与 LAN 抽取（E5）需真机/双进程联调，建议各自配 e2e。
- 全程保持 `rdg remote --daemon` / `rdg tmux` 向后兼容。

## 4. 验收总览

- 每步 `cargo check`/`cargo test` 绿；
- `rdg` 无参→TUI；TUI 覆盖 登录/配对/公网host/公网控制/LAN（E2/E4/E5）；
- 渲染语义与桌面 §B.9 一致（自然宽度、允许溢出、不补尾随空格）；
- 桌面 LAN/公网远控回归不破。
