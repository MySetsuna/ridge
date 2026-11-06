# 10 — Kernel 与业务服务

[server.rs](../../packages/ridge-kernel/src/server.rs)组合 AppState、路由和服务，[lifecycle.rs](../../packages/ridge-kernel/src/lifecycle.rs)负责发现及启动独立进程。职责依据见[统一 Runtime](../../specs/l2-unified-client-runtime.md)。

## 请求与状态

[host.rs](../../packages/ridge-kernel/src/host.rs)中的 KernelHost 统一本地展示和 Remote 请求，维护连接级选择及订阅。共享业务入口经受保护的 /v1/domain/commands 进入 [business.rs](../../packages/ridge-kernel/src/business.rs)，调用领域服务及 [ridge-core](../../packages/ridge-core/src/lib.rs)。Host 的部分领域访问仍经内部 HTTP 请求。

[domain.rs](../../packages/ridge-kernel/src/domain.rs)维护工作区图和 PTY 操作。[pty.rs](../../packages/ridge-kernel/src/pty.rs)的 PtyRegistry 拥有 child、写入、resize、退出状态、有界输出及权威语义生产。reader 将字节送入 TerminalProducer::feed_semantic，服务提供终端快照、delta 与历史。

Runtime 持有工作区/pane 身份、PTY、Host 连接、Commune 及持久化服务。客户端选择、焦点、滚动与渲染镜像属于展示状态。显式 .ridge 文件由[工作区服务](../../packages/ridge-kernel/src/workspace_files.rs)管理，临时运行态与活进程另行维护。[Cloud manager](../../packages/ridge-kernel/src/cloud/manager.rs)及 [LAN 控制](../../packages/ridge-kernel/src/remote_control.rs)由 Runtime 管理。

独立终端创建在服务内串行校验身份、启动 PTY、提交拓扑，保存失败销毁新 PTY。Host snapshot 读取无隐式创建。条件见 [CLI 创建契约](../../specs/l3-cli-independent-terminal-creation.md)。

## 核验入口

[headless_runtime](../../packages/ridge-kernel/tests/headless_runtime.rs)、[host_topology_runtime](../../packages/ridge-kernel/tests/host_topology_runtime.rs)及 [runtime_workspace_continuity](../../packages/ridge-kernel/tests/runtime_workspace_continuity.rs)覆盖独立服务与多客户端。
