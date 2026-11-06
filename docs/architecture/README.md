# Ridge 架构导览

本目录解释当前源码与流程。权威需求位于根目录 [specs](../../specs/)，变更位于 [changes](../../changes/)，均经 SpecTree 提案、批准、锁定、验证与完成流程管理。

## 当前阅读入口

| 章节 | 内容 |
| --- | --- |
| [系统总览](00-overview.md) | 独立 Runtime、展示端与传输边界 |
| [Kernel](10-kernel.md) | 业务组合、PTY、持久化与事件 |
| [终端协议](20-rtp1-wire.md) | 当前语义协议、生成源与 RTP1 兼容面 |
| [连接生命周期](30-remote-lifecycle.md) | 订阅、恢复与进程存活 |
| [CLI](40-cli.md) | TUI、仪表盘、LAN、Cloud 与独立终端创建 |
| [桌面](50-desktop-tauri.md) | OS bootstrap、共享 client 与渲染 |
| [数据流](60-data-flow.md) | 输入、输出、工作区与 Commune |
| [身份与代际](70-runtime-epoch.md) | Host、Runtime、pane 与客户端选择 |
| [安全](80-security.md) | 私有凭据、本地授权与 Remote 权限 |
| [性能与验证](90-performance.md) | 可执行入口及证据边界 |

## 权威契约

- [统一客户端 Runtime](../../specs/l2-unified-client-runtime.md)规定服务所有权与展示边界。
- [CLI 独立终端创建](../../specs/l3-cli-independent-terminal-creation.md)规定四入口的鉴权、提交与重试。
- [Remote 交互可靠性](../../specs/l3-remote-interaction-reliability.md)规定当前终端语义与恢复。
- [Commune 协议成员](../../specs/l3-commune-protocol-membership.md)规定 Host 全局成员、编组与历史。
- [协议](../../specs/L2-PROTO-001.md)、[终端](../../specs/L2-TERM-001.md)、[生命周期](../../specs/L2-REMOTE-001.md)、[性能](../../specs/L2-PERF-001.md)须结合后续修订阅读。

## 历史材料

CHG-089 改写前的十一篇导览原文保存在 [Foundation 导览归档](../archive/CHG-089/architecture/)。旧版将 RTP1 adapter、Tauri 模块和阶段测试数量作为当时依据。

[Foundation 终报告](RIDGE-RUNTIME-FOUNDATION-FINAL.md)、[验收记录](notes/README.md)、本目录 [旧规范副本](specs/)及 images/ 中四张 SVG 均为历史材料。其原始结论、相对链接与图片只表达当时状态，当前入口以本页所链根规范及实现为准。
