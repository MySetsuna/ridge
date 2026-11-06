# 60 — 端到端数据流

## 终端输入与输出

```text
键盘 / TUI 输入
  → 本地授权 WS / LAN WSS / Cloud WebRTC
  → KernelHost 校验连接、pane 与订阅
  → Kernel PTY 写入 → child
child 输出
  → PtyRegistry reader → TerminalProducer.feed_semantic
  → 权威 snapshot / delta / history
  → Host 活动订阅 → 传输适配
  → 客户端校验代际与 revision → 展示镜像 → 渲染
```

真实接线见 [pty.rs](../../packages/ridge-kernel/src/pty.rs)与 [host.rs](../../packages/ridge-kernel/src/host.rs)，前端消费见 [manager.ts](../../packages/remote/src/shared/terminal/manager.ts)，TUI 消费见 [lan_session.rs](../../packages/ridge-cli/src/tui/lan_session.rs)。有界队列与恢复以[现行终端契约](../../specs/l3-remote-interaction-reliability.md)为准。

## 业务与事件

共同 Host 请求到受保护的 /v1/domain/commands，由 [business.rs](../../packages/ridge-kernel/src/business.rs)按 native/Remote 身份、方法和路径授权，交给领域服务及 ridge-core。Runtime 产生状态事件，客户端更新投影。Host 部分内部调用仍经私有 HTTP 和事件 WS。

创建为显式写操作，Host snapshot 读取无初始创建副作用。[CLI 四入口](40-cli.md)共享独立终端创建。保存 .ridge 属显式持久化，运行态与客户端选择各自维护。

## Commune

[Commune 契约](../../specs/l3-commune-protocol-membership.md)规定 Host 全局成员及编组。Runtime 持有成员、协议连接、任务收据与历史，桌面/Remote 展示投影。内部按 workspace/pane/session 精确绑定，编组持久化含 revision 校验与迁移，任务 Hub 收据归服务端。

[commune_connection.rs](../../packages/ridge-kernel/src/commune_connection.rs)接协议连接，[成员面板](../../src/lib/teammate/)接事件。新消息按当前视图与历史位置处理，历史导航及未读提示各有语义。核验见 [commune_registration](../../packages/ridge-kernel/tests/commune_registration.rs)、[commune_group_tasks](../../packages/ridge-kernel/tests/commune_group_tasks.rs)及 [live-history 验收](../../scripts/commune-live-history-acceptance.mjs)。
