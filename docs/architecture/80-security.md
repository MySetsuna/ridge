# 80 — 鉴权与可信执行边界

权限依据[统一 Runtime](../../specs/l2-unified-client-runtime.md)、[CLI 创建](../../specs/l3-cli-independent-terminal-creation.md)及[Commune](../../specs/l3-commune-protocol-membership.md)。共同 handler 保留 native、LAN、Cloud 的信任差异。

## 本地连接

[server.rs](../../packages/ridge-kernel/src/server.rs)保护私有控制入口。OS bootstrap 使用私有凭据申请 [local_client.rs](../../packages/ridge-kernel/src/local_client.rs)定义的独立、限时、绑定 origin/device 的 grant。

[runtimeClient.ts](../../src/lib/transport/runtimeClient.ts)核验回环地址、路径、origin 与协议，以 WS 子协议提交 token，凭据仅留内存。服务校验回环 peer、Origin、device、token 和到期状态。本地 grant、LAN 普通会话与 Kernel 私有认证各自隔离。

## 授权与失败

[host.rs](../../packages/ridge-kernel/src/host.rs)与 [business.rs](../../packages/ridge-kernel/src/business.rs)校验身份、方法、目标与路径。前端 [remoteAllowlist](../../packages/remote/src/shared/cloud/remoteAllowlist.ts)表达调用集合，服务端 [capability.rs](../../packages/ridge-core/src/capability.rs)及业务校验保证权限。

独立终端初始创建限定 native。LAN/Cloud 客户端提交时拒绝且无新工作区/PTY。TUI 先获有效 grant 并建立 WS，Cloud 显式初始创建还需认证 Host welcome。

无效、过期、重放、跨身份或失效代际请求明确失败。未知提交结果保留身份核对重试，持久化失败回滚新 PTY。验收见 [remote_auth_runtime](../../packages/ridge-kernel/tests/remote_auth_runtime.rs)、[local_client](../../packages/ridge-kernel/tests/local_client.rs)、[CLI 创建验收](../../packages/ridge-cli/tests/cli_terminal_create_e2e.rs)与 [ridge-remote auth](../../packages/ridge-remote/src/auth.rs)。

LAN 使用 WSS 适配，Cloud 由 Runtime 的 [Cloud 服务](../../packages/ridge-kernel/src/cloud/manager.rs)管理认证信令与加密连接。版本、限额与字段查[生成契约](20-rtp1-wire.md)。
