//! PTY 输出字节流的**纯解析层**（runtime-agnostic，**zero Tauri / zero AppState**）。
//!
//! 这一层把「PTY 读出来的原始字节」转成**语义信号**，不持有任何 host 状态、不
//! 触发任何事件，纯输入→输出，因此桌面（Tauri `engine::pty`）与无头
//! `ridge-cli` host 可以共用同一份实现（契约 §9 / §11.D，D11 的可安全抽取切片）。
//!
//! 桌面端 `src-tauri/src/engine/pty.rs` 的读线程仍然保留**与 `AppState` 绑定的
//! 编排**（scrollback、`event_tx` 路由、teammate 生命周期、resize 静默窗口策略
//! 等），只把下面这些纯函数下沉到本模块并委托调用：
//!
//! - [`decode`] —— PTY 字节块的**增量 UTF-8 解码**（跨 chunk 残字节缓存）。
//! - [`prompt`] —— shell-integration **prompt OSC**（FinalTerm `OSC 133` / VS Code
//!   `OSC 633`）起始偏移扫描。
//! - [`cwd`] —— **OSC 7** 工作目录探测。
//! - [`title`] —— **OSC 0/1/2** 窗口标题探测。
//! - [`chunk`] —— 把以上拼成**每块归约**（ConPTY resize 静默门 + 信号扫描），
//!   即读循环里与 AppState 无关的核心。
//!
//! 这些解析器原本散落在 `engine::{cwd,title,pty}` 且 `engine` 是 `src-tauri` 的
//! crate-private 模块，外部 crate（`ridge-cli`）不可见；下沉到 `ridge-core` 后，
//! 无头 host 也能据此向 controller 上报 cwd / title / prompt 信号。

pub mod chunk;
pub mod cwd;
pub mod decode;
pub mod prompt;
pub mod title;
