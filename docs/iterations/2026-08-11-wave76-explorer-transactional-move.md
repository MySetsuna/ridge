# Wave 76：Explorer 跨卷移动事务化（2026-08-11）

## 来源与结论

本轮重新完成 NLM 认证，访问使用固定代理 `http://127.0.0.1:51081`；读取已登录笔记的既有对话 `a47d3199-c1f9-47f1-927c-ff2c4875b77d`，未修改 NotebookLM 笔记或发起发布。近期对话把 `REQ-EXPLORER-FILE-CONTINUITY-01` 的未闭环点收敛为：跨卷移动在复制中途失败，或复制成功后源删除失败时，磁盘、目标树和 cut clipboard 可能分叉。

本地代码核验确认该风险真实存在：旧 `move_path` 直接把源复制到最终目标，再删除源；递归 `copy_directory` 中途失败会留下最终目标半成品，源删除失败则可能形成源/目标双份。Explorer 的失败 DTO 只能保留失败源路径，不能回滚已落到最终目标的半成品。

## 修复

`packages/ridge-core/src/fs/commands.rs` 的跨卷 fallback 现采用临时路径事务：

1. 在目标父目录生成带进程号与 UUID 的隐藏 staging 路径。
2. 先完整复制到 staging；复制失败清理 staging，源保持不变。
3. staging 成功后重命名到最终目标；提交失败清理 staging，源保持不变。
4. 删除源；删除失败时移除已提交目标并返回“已回滚”证据。
5. 若回滚也失败，错误同时保留源删除与回滚失败原因，避免伪称操作完成。

同卷移动仍先走原子 `rename` 快速路径；Tauri `move_path` wrapper 与 Explorer DTO/clipboard 语义未改变。

## 确定性验证

- `cargo fmt --all -- --check`：通过。
- `cargo test -p ridge-core --lib fs::commands::tests --quiet`：21/21。
- `cargo test -p ridge-core --lib --quiet`：344/344。
- `codegraph sync`：通过；随后复核 `move_path → move_via_staging → copy/remove/rename` 及 Tauri wrapper 调用链。
- 新增测试覆盖：同卷快速 rename、源/目标前置校验、复制失败及半成品清理、临时清理失败、提交失败、源删除失败后的目标回滚、回滚失败。

## dev:cdp / 现场验证

- `pnpm cdp:smoke`：通过；动态 CDP 端口为 `7521`。
- `node scripts/cdp-cross-volume-e2e.mjs`：通过，C→D→C 往返 `27` bytes。
- `node scripts/cdp-cross-volume-acl-e2e.mjs`：通过；复制成功后 ACL 拒绝源删除，目标可读且源保留。
- `$env:NODE_TLS_REJECT_UNAUTHORIZED='0'; node scripts/cdp-lan-probe.mjs`：通过；hello、pane UUID、scrollback/live frame、echo、pong 全收齐。未将关闭 TLS 校验作为产品修复。
- `node scripts/cdp-dpr-e2e.mjs`：通过，DPR `1.5`，3/3 canvas backing canvas 对齐。
- `pnpm cdp:pty`：失败；本次为 `ws error`、`closed before pane (1006)`、`hard timeout`，与此前冷启动/ConPTY harness 不稳定一致，未据此判定本轮 FS 改动有误。
- 手机 Agent probe：未配置 `RIDGE_REMOTE_CA_CERT` 时先因自签 CA 校验失败；补充 `$env:LOCALAPPDATA\\ridge\\remote-tls\\ca.pem` 后 120 秒未收敛，列为下一轮 mobile TLS/harness bug。

## 尚未宣称闭环

- 当前单元测试以注入故障确定性验证事务分支；本机仍缺“真实跨卷复制成功后，外部 ACL/锁在源删除窗口介入”的稳定物理复现。既有 ACL E2E 仅证明拒绝时源保留，不能替代该中窗故障证据。
- SonarQube 服务器可用且最后成功分析的 Quality Gate 为 `OK`，但本轮新扫描曾超时，故不把旧指标记作本轮扫描结果。
- NLM 对话中另列的第三方 Runtime/A2A、公网/TURN、双真实窗口、实体移动设备、物理 DPR 与 PTY 五条件现场证据，仍按 ACTIVE 管理；本轮未伪造其完成状态。

发布、push、tag、Release 未执行；凭据未写入仓库。
