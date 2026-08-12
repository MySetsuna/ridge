# A2A 标准适配与质量阀收口（2026-08-12）

## 结论

本轮完成 Ridge 内部 Message Hub 到外部 A2A Agent 的标准客户端适配。Ridge 的
Hub 仍是内部消息事实源；A2A 仅作跨系统外部适配器，不替代 Hub、Kernel
Runtime API 或高频事件总线。

“Ridge 自身作为 A2A 服务端发布 Agent Card、接受第三方调用”不在本轮代码中，
也没有用本地 fixture 冒充第三方互操作完成。真实第三方 Agent Card、凭据、网络
策略与外部设备仍须在目标环境验证。

## NotebookLM 近期痛点对账

NotebookLM 近期对话被作为假设来源；代码、CodeGraph、确定性测试与运行证据优先。
对账结果如下：

| 主题 | 本轮结论 |
| --- | --- |
| Message Hub / Agent 通信 | Kernel/Teammate SSOT、typed envelope、SQLite Hub、generation/lease fencing 与 MCP 语义工具保持有效；A2A 落在 Hub 外部 adapter 层。 |
| ridge-mcp | 继续作为 Kernel API/语义控制面；不直接写 PTY。新增 A2A 注册、发现、投递与 receipt 映射。 |
| pane / ridge-term | CDP smoke、multitab、PTY parser 与 pane graph 复跑通过；远端关闭 pane 的结构广播已移入权威 close 路径，避免调用方漏发。 |
| workspace / remote | LAN desktop/mobile E2E 通过；跨设备、公网/TURN、真实手机与物理 DPR 仍属外部证据。 |
| Sonar | 本地 SonarQube 扫描完成，最新 CE task 成功，Quality Gate 为 OK。 |

## A2A 实现范围

`packages/ridge-mcp/src/a2a.rs` 提供边界受控的 blocking HTTP A2A client：

- Agent Card 发现、JSON-RPC interface 选择与相对 endpoint 解析；支持优先版本，
  兼容 A2A `1.0` 标准方法名与 `0.3` legacy 方法名。
- JSON-RPC 2.0 请求/响应校验：id、error、`SendMessage` task/message one-of、
  SSE stream 的 task/message/statusUpdate/artifactUpdate one-of。
- 标准 `Content-Type`、`Accept`、`A2A-Version`、`A2A-Extensions`、Bearer
  Authorization；Agent Card interface 的 tenant 写入 params，不使用非标准 tenant
  请求头。
- `SendMessage`、stream、task get/list/cancel、task resubscribe、push notification
  config CRUD、extended Agent Card；按 Agent Card capability fail-closed。
- Message role、messageId、parts 与 part content one-of 校验；响应体、SSE 行、事件
  数量与文本均有上限。
- `A2aEndpointRegistry` 以 `(agentId, generation, lease)` 做注册、投递与注销围栏；
  旧 generation/lease 不得复用路由或产生成功 receipt。
- Hub entry 转换为 A2A user Message；本地 Hub `taskId` 不会泄漏到远端，只有显式
  `payload.a2aTaskId` 才映射为 A2A `taskId`；远端 task/message id 回写 delivery
  receipt。

MCP 暴露：

- `ridge_register_a2a_endpoint`
- `ridge_unregister_a2a_endpoint`
- `ridge_send_message` 的可选 `a2a_task_id`

生产 Kernel、tmux 与 Tauri teammate host 均走统一 A2A delivery route；旧本地
DeliveryRegistry 仍作为兼容 fallback。

## 确定性验证

- `cargo test -p ridge-mcp --lib --quiet`：100 passed。
- `cargo clippy -p ridge-mcp --all-targets --quiet`：通过。
- `cargo test -p ridge-kernel --lib --quiet`：50 passed。
- `cargo test -p ridge-tmux --lib --quiet`：11 passed。
- `cargo check --manifest-path src-tauri/Cargo.toml --lib --quiet`：编译通过；仅有既有 unused/dead-code warnings。
- `pnpm check`：0 errors / 0 warnings。
- `pnpm test`：217 files，2013 passed，5 skipped。
- `pnpm build`、`pnpm build:remote`、`pnpm verify:pwa`：通过。
- 最新 `dev:cdp`：smoke、multitab、PTY parser、pane graph（三次独立复跑）均通过。
- `pnpm e2e:rdg-lan`：项目 CA 下 desktop/mobile 均 PASS，verify、WebSocket、输入、resize 均通过。

### 已修复的运行 bug

1. A2A JSON 请求曾将 `Accept` 值误传为 header 名 `accept`；现为标准
   `Accept: application/json`，并有真实 HTTP fixture 断言。
2. `remote_close_pane` 只在部分 WS 调用方广播，teammate MCP 路径会留下 stale
   LAN panes frame；现由权威 close 函数统一广播，WS 调用方不重复广播。
3. CDP PTY parser 的 `createRequested` 读取了错误作用域；已改为状态对象字段并补
   parser state tests。
4. SSR 最终 HTML 生成后的 CSP hash 曾遗漏；现由 server transform 对最终 HTML
   同步 CSP，换行按浏览器 tokenization 规范归一化。

## 尚未闭合的外部边界

- Ridge 原生 A2A server/Agent Card endpoint：未实现，不能声称“第三方可直接调用 Ridge”。
- 真实第三方 Agent Card 的 v1/legacy、stream、push、tenant 与 auth 互操作：需要
  外部端点与凭据；本仓 fixture 仅证明 adapter 的协议分支。
- PTY 五条件的真实生产原子快照、实体手机/PWA/IME/后台恢复、物理 DPR/像素矩阵、
  公网 TURN、跨卷 ACL 与真实 Agent CLI 私有 Runtime：继续 ACTIVE。

## 质量阀

SonarQube `http://127.0.0.1:9000` 状态 UP，项目 `MySetsuna_ridge` 最新分析：

- CE task：`SUCCESS`
- Quality Gate：`OK`
- new coverage：`84.4%`（阈值 `80%`）
- new duplicated lines density：`1.21897%`（阈值 `3%`）
- new violations：`0`

最新 CE task：`8e2a4c70-8030-4ee0-83bb-13cb967da8d757`；analysis：`73babef4-c944-45ea-a22b-db42977fbad6`。

扫描使用一次性 token；扫描结束后已撤销。密码、Cookie、token 不入库。

## 发布边界

版本已升至 `0.1.62`，待工作区运行态产物清理、提交、推送与 CI matrix 产物齐全
后再以 annotated tag `v0.1.62` 发布。缺少任一平台安装包时，不宣称 Release 完成。
