# Wave 87：Rust crate 质量收口与 Sonar 交接（2026-08-11）

## 本轮完成

针对本地 `cargo clippy --all-targets -- -D warnings` 暴露的可确定修复项，完成以下最小质量修正：

- `ridge-term`：修正切片拷贝、迭代器/API 习惯用法、宽字符边界判断、测试断言、文档列表与 WASM API 的合法参数例外；保留现有协议和导出签名。
- `ridge-mcp`：改用单次查找、借用切片、直接 `contains` 与正确的 WebSocket 文本类型，避免重复分配。
- `ridge-core`：收敛 Git 解析返回类型、修正双端迭代器、错误构造、UTF-8 pending 取空、测试锁跨 await 误报及文档/断言问题。
- `ridge-remote`、`ridge-kernel`、`ridge-tmux`：补齐可推导 `Default`/`is_empty`、显式文件截断语义、默认结构初始化，以及保留既有 API 形态所需的局部 Clippy 例外。

未改第三方 `ridge-signaling` / `ts-rs`；其 serde attribute 解析告警来自外部依赖，仍需依赖升级或上游修复。

## 验证

- `cargo clippy -p ridge-term --all-targets -- -D warnings`：通过。
- `cargo clippy -p ridge-core --all-targets -- -D warnings`：通过。
- `cargo clippy -p ridge-mcp --all-targets -- -D warnings`：通过。
- `cargo clippy -p ridge-remote -p ridge-kernel --all-targets -- -D warnings`：通过。
- `cargo clippy -p ridge-tmux --all-targets -- -D warnings`：通过。
- `cargo test -p ridge-term --lib --quiet`：401 passed。
- `cargo test -p ridge-core --lib --quiet`：344 passed。
- `cargo test -p ridge-mcp --lib --quiet`：90 passed。
- `cargo test -p ridge-remote --lib --quiet`：28 passed。
- `cargo test -p ridge-kernel --lib --quiet`：50 passed。
- `cargo test -p ridge-tmux --lib --quiet`：11 passed。
- `cargo fmt --all`、相关 `git diff --check`：通过。

全 workspace `cargo clippy --workspace --all-targets -- -D warnings` 尚未通过：进入 `src-tauri` 主 crate 后仍有约 200 条备用/测试路径 `dead_code` 与未使用项，以及少量风格项。它不是当前 Sonar Quality Gate 的服务端证据，未以局部 crate 绿结果冒充全仓绿门。

## Sonar 交接

项目卡仍显示旧分析 `c271e74b-ac3f-4277-bbef-74418f48b822` 的 `Failed`：该分析当时 `new_coverage=80.1%`、重复率通过，但 `new_violations=1`，所以 Quality Gate 为 `ERROR`。本轮已修复旧 Rust 复杂度问题并移除无效 `packages/rg-split/examples/tsconfig.json` 配置；必须用有效 token 重新上传，确认 CE 成功、项目 coverage ≥80%、new issues/violations=0、Quality Gate `OK` 后，页面才会更新。

本轮未保存或提交凭据，未 push/tag/release。扫描日志均为 `.tools/` 运行态，不纳入提交。

交接时只读复核 `http://127.0.0.1:9000` 已连接拒绝；这进一步解释页面不会刷新，需先恢复 Sonar 服务，再注入有效 token 执行新分析。
