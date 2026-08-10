# Wave 78：rdg PTY metadata 发送边界（2026-08-11）

## 变更

仍在运行的 Remote 子任务留下 `packages/ridge-cli/src/tui/lan_host_impl.rs` 未提交改动；经审计后纳入本轮。`start_pane_subscription` 原先内联解析订阅字段、发送 raw binary 与 `pty-meta`，现拆为三个可验证边界：

- `pane_subscription_request`：统一读取顶层或 `params` 中的 `paneId` / `resume`，无效 pane fail-closed。
- `send_pane_bytes`：先发 pane binary，再发对应 metadata，保持顺序。
- `send_pane_metadata`：复用 UTF-8/OSC carryover；无 metadata 为 no-op，发送通道关闭返回失败。

该变更整理 rdg Remote 的 metadata 发送出口，降低订阅回放与 live feed 两条分支的漂移风险；不宣称其单独解决公网、TURN、真机或第三方 Remote 接入问题。

## 验证

- `cargo fmt --all -- --check`：通过。
- `cargo test -p ridge-cli --bin rdg tui::lan_host_impl::tests --quiet`：19/19。
- `cargo test -p ridge-cli --bin rdg --quiet`：158/158。
- `codegraph sync` 后复核：`start_pane_subscription → send_pane_metadata/send_pane_bytes → rdg_metadata_frame`。
- 新增测试覆盖：顶层/嵌套订阅字段、缺失/非法 pane、binary→metadata 顺序、输出通道关闭。

## 边界

本轮 dev:cdp 的 LAN probe 在显式测试 TLS 配置下通过，但该 rdg sidecar 改动发生于此前运行实例之后，未将旧实例结果冒充新二进制现场证据；下一次 dev:cdp 重启需复跑 LAN/PTY/移动四路径。公网凭据、实体手机、第三方 Runtime/A2A 与 Sonar 新扫描仍未闭环。
