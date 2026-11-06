# 20 — 当前终端协议与 RTP1 兼容面

保留本文件路径以维持既有引用。现行行为依据[Remote 交互可靠性](../../specs/l3-remote-interaction-reliability.md)及根[协议规范的终端契约修订](../../specs/L2-PROTO-001.md)。

## 当前主链

PTY 字节在 Kernel 内解析成权威终端语义。Host 将 pane 绑定的 snapshot、delta、历史及控制结果发送给客户端。展示端校验身份、代际与 revision，更新渲染镜像。消息字段、版本、限制与错误查权威 schema 及生成表示。

- [sync-signaling.mjs](../../scripts/sync-signaling.mjs)同步外部 ridge-signaling 契约。
- [SOURCE_REV](../../packages/remote/src/shared/cloud/signaling/SOURCE_REV)记录当前同步版本。
- [Rust 生成表示](../../packages/ridge-term/src/terminal_protocol_generated.rs)由 ridge-term 暴露为 terminal_protocol。
- [前端生成表示](../../packages/remote/src/shared/cloud/signaling/generated/)供共享传输与消费侧使用。
- [producer.rs](../../packages/ridge-term/src/producer.rs)产生语义状态，[pty.rs](../../packages/ridge-kernel/src/pty.rs)接真实 PTY。

本地授权 WS、LAN WSS 与 Cloud WebRTC 承载共同业务/终端语义，各自保留鉴权和传输边界。

## 历史与兼容

[server.rs](../../packages/ridge-kernel/src/server.rs)仍注册 /v1/rtp1，[rtp1.rs](../../packages/ridge-kernel/src/rtp1.rs)保留兼容实现。旧 CLI rtp1_kernel_client.rs 与 pty.rs 的存在不能作为现役 TUI 调用依据。当前 TUI 经 [lan_session.rs](../../packages/ridge-cli/src/tui/lan_session.rs)消费共同 Host。

Foundation 的 22 消息与 adapter 见[旧导览](../archive/CHG-089/architecture/20-rtp1-wire.md)。兼容回归见 [conformance_rtp1](../../packages/ridge-kernel/tests/conformance_rtp1.rs)，当前终端恢复见 [snapshot_history_runtime](../../packages/ridge-kernel/tests/snapshot_history_runtime.rs)。
