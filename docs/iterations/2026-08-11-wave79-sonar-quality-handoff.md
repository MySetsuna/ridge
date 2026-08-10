# Wave 79：Sonar 复核与 RDG 订阅复杂度收口（2026-08-11）

## 已完成

- `packages/ridge-cli/src/tui/lan_host_impl.rs:589` 起，将 pane 订阅请求解析、metadata 发送、binary/live 输出拆为独立边界，消除本轮 Sonar `rust:S3776` 认知复杂度告警。
- 提交：`254a98e3 refactor: isolate rdg pane metadata delivery`。
- `cargo fmt --all -- --check`：通过。
- `cargo test -p ridge-cli --bin rdg tui::lan_host_impl::tests --quiet`：20 passed、0 failed。
- `cargo test -p ridge-cli --bin rdg --quiet`：158 passed、0 failed。

## Sonar 现状

本轮已成功上传的分析任务为 `c271e74b-ac3f-4277-bbef-74418f48b822`。其结果为：

- Quality Gate：`ERROR`。
- 新覆盖率：`80.1%`，阈值 `80%`，通过。
- 新重复率：`0.84181%`，阈值 `3%`，通过。
- 新违规：`1`，阈值 `0`，失败；违规为 `rust:S3776`，位置 `packages/ridge-cli/src/tui/lan_host_impl.rs:589`。
- 当时遗留未闭合问题另有 `src/remote/index.html` 的 `Web:PageWithoutTitleCheck`；当前源码已含 `<title>Ridge Remote - Agent Terminal</title>`，需下一次有效分析确认是否关闭。

复杂度修复后尚未取得新的 Quality Gate 证据：本机以用户提供的账号信息校验 Sonar 登录，`admin` 及消息中的拼写均返回 `401` / `valid=false`。未猜测密码、未保存密码、未输出令牌；待有效 Sonar 凭据注入后，按既有产品源码范围重跑：

```powershell
sonar-scanner.bat -Dsonar.sources=src,packages,src-tauri/src -Dsonar.qualitygate.wait=true -Dsonar.qualitygate.timeout=300
```

复核应同时确认：`new_violations=0`、`new_coverage>=80`、`new_duplicated_lines_density<=3`，以及未解决问题列表为空。`.tools/sonar-scan-wave77b*.log` 仅为本地运行日志，不纳入提交。

## 边界

当前已完成本轮可本地验证的代码与测试；Sonar 最终 Gate、旧 HTML 问题关闭状态及全量构建仍依赖有效 Sonar 认证与运行环境，不能以本地测试结果替代。
