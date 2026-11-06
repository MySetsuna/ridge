# 50 — 桌面展示与 OS 适配

[lib.rs](../../src-tauri/src/lib.rs)的 command 注册清单限定为 Runtime bootstrap、启动上下文、系统文件展示、剪贴板及 Deep Root OS 操作。业务所有权依据[统一 Runtime](../../specs/l2-unified-client-runtime.md)。

## 启动链

1. [kernel_lifecycle.rs](../../src-tauri/src/kernel_lifecycle.rs)发现或受限启动独立 Kernel，申请本地 client grant。
2. [布局入口](../../src/routes/+layout.svelte)调用 [runtimeClient.ts](../../src/lib/transport/runtimeClient.ts)核验 grant 地址、origin、身份与协议。
3. WebView 以 ridge-client 及授权子协议打开 /v1/local/ws，凭据保留内存。
4. RemoteConnection 与共享 WS adapter 提供业务 RPC/事件，[tauriShim](../../src/lib/transport/tauriShim/)保留兼容调用表面，业务路由至共同 Host。

历史类型名或 shim 路径仍可能保留，具体所有权以服务调用和状态写入为准。

## 展示与保存

终端消费共享语义，展示层维护 WebGPU 渲染、焦点、选择与滚动镜像。文件/Git/设置、PTY、Host 和 Commune 操作经服务授权。窗口、托盘、对话框及剪贴板由 OS 适配。

关闭桌面仅断开客户端，重开绑定现存工作区与 PTY。.ridge 显式保存及恢复由 Runtime 管理，临时工作区属于当前运行态，冷启动布局恢复须显示进程真实状态。

[local_client](../../packages/ridge-kernel/tests/local_client.rs)核验本地授权，[runtime_workspace_continuity](../../packages/ridge-kernel/tests/runtime_workspace_continuity.rs)核验连接隔离与继续运行，[bridge.test.ts](../../src/lib/transport/tauriShim/bridge.test.ts)核验前端桥。真实桌面关闭/重开与渲染结论须另查实际验收证据。
