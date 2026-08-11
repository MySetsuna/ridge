# NLM 下一轮需求落地复核（2026-08-11）

## 结论

本轮从 NotebookLM 笔记与近期对话抽取的可本地验证项已落地并提交前复核：pane/term resize、workspace/CWD、teammate split 生命周期、Remote capability/allowlist、Cloud E2E harness、CDP 隔离 profile/target 与本地 Cloud 启动参数均有源码及测试证据。

仍未闭合者，不作“已完成”宣称：

- 公网 relay/WebRTC/TURN/E2EE/reconnect 真实网络矩阵；
- 实体 iOS/Android 的 IME、visualViewport、后台恢复；
- Windows PowerShell/PTY 的真实 DPI 矩阵；
- 双真实窗口/多 host focus 现场证据；
- NTFS 跨卷/权限真实矩阵；
- Sonar 新扫描与 Quality Gate：本机 Sonar 服务可达；凭证已验证可用，但本轮 CE 任务因 `Fail to execute es request` 失败，Gate 为 `ERROR`，不能作为代码质量通过证据。

## 本轮落地

- `ridge-term`：resize/reflow、宽字符边界、inline TUI/alt-screen 分支拆分，补齐重排辅助路径；
- `ridge-core` / `src-tauri`：进程 CWD 跨平台实现、PTY resize/prepare、remote writer lane、event forwarder 与 teammate split 边界整理；
- Remote/Cloud：workspace scope、capability/allowlist、host/controller bridge 及 pane stream 逻辑保留 fail-closed；
- CDP：支持隔离 `RIDGE_CDP_USER_DATA_DIR`、`RIDGE_CDP_TARGET_DIR`；本地 Cloud CDP 参数带引号的 tenant resolver，并显式使用 `--no-proxy-server`，公网 base 不注入该参数；
- CLI：dashboard 登录流程正确消费 `AuthFile` 后返回 unit result。

## 证据

- `codegraph sync`：通过；并复核 `get_process_cwd`、`resize_screens`、`spawn_ws_writer`、`spawn_event_forwarder`、`route_split` 调用图；
- `pnpm check`：0 errors / 0 warnings；
- `pnpm test`：216 files，2000 passed，5 skipped；
- Remote/Cloud/hosts/transport/CDP 定向 Vitest：46 files，559 passed，1 skipped；
- `cargo test -p ridge-core --lib`：344 passed；
- `cargo test -p ridge-term --lib`：401 passed；
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`：285 passed；
- `cargo check -p ridge-cli --quiet`：通过；
- `cargo fmt --all -- --check`：通过；
- `pnpm cdp:smoke`：通过，WebView2 CDP 动态端口可连接；
- 本地 `ridge-cloud` health：HTTP 200；Node 侧带真实 seed token 的 WebSocket：可建立；
- Tauri WebView2 真实 Cloud E2E：仍失败于 tenant WebSocket，日志为 `NETWORK: 信令 WebSocket 错误`。已确认 CDP 进程实际带 `--host-resolver-rules` 与 `--no-proxy-server`；故该项仍为待修复/待现场复核 bug，不计入通过。
- `pnpm build:remote`：释放隔离 CDP 资源后仍在 240 秒内未完成，当前只能记录为构建超时，未宣称通过。

## Sonar

本机 SonarQube 已安装并运行于 `http://127.0.0.1:9000`。本轮 scanner 已成功提交 CE 任务，但两次任务均失败：`Fail to execute es request`；API 返回的现存 Gate 快照为 `ERROR`，其中 `new_coverage=79.7/80`、`new_violations=10/0`。因失败任务没有 `analysisId`，该 Gate 数值仅作服务现状记录，不作本轮代码结论。这不是认证失败；应先修复/重启 Sonar Elasticsearch 或缩小扫描范围，再复扫。凭据不写入仓库，交接时通过安全渠道提供。

## 发布边界

未 push、未 tag、未 Release、未发布 Remote/Cloud artifact；发布仍须用户单独授权。
