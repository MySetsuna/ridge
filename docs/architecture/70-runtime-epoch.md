# 70 — 身份、代际与客户端选择

身份与恢复依据[统一 Runtime](../../specs/l2-unified-client-runtime.md)及[现行终端契约](../../specs/l3-remote-interaction-reliability.md)，具体字段查生成协议。

## Runtime 身份

[server.rs](../../packages/ridge-kernel/src/server.rs)每次启动生成 runtime_epoch，当前为 UUID v7。host_id 优先取 RIDGE_HOST_ID，默认含主机名与 epoch，不能由字段名称推断跨重启稳定性。Cloud/设备签名身份另由[身份服务](../../packages/ridge-kernel/src/controller_identity.rs)管理。

[registry.rs](../../packages/ridge-kernel/src/registry.rs)与 [lifecycle.rs](../../packages/ridge-kernel/src/lifecycle.rs)提供发现及连接检查。端口与 PID 属定位证据，客户端须核验 endpoint、健康与代际。新 Runtime 拒绝旧代际创建及终端操作。

| 状态 | 边界 |
| --- | --- |
| workspaceId / paneId | 服务持有的实体身份 |
| terminal lifetime / activation / revision | 隔离旧进程、旧订阅和乱序状态 |
| history generation | 防止过期历史页改写当前窗口 |
| 连接所选工作区 | Host 按连接隔离 |
| 焦点、滚动与选区 | 客户端展示状态 |

[Host](../../packages/ridge-kernel/src/host.rs)管理连接选择，[terminal_lifecycle](../../packages/ridge-kernel/src/terminal_lifecycle.rs)管理相关终端状态。其他客户端选择变化不能改写本连接目标。

CLI 为一次逻辑创建固定 workspaceId、paneId、runtimeEpoch，再次显式创建生成新身份。冲突及关闭后拒绝条件见[创建契约](../../specs/l3-cli-independent-terminal-creation.md)。
