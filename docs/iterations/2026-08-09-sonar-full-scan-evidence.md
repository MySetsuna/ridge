# Sonar 全项目扫描证据（2026-08-09）

## 扫描事实

- 项目：`MySetsuna_ridge`
- SonarQube：`http://127.0.0.1:9000`，版本 `26.7.0.124771`
- Scanner：`.tools/sonar-scanner-8.0.1.6346-windows-x64`
- 扫描配置：仓库 `sonar-project.properties`，未改用定向 `sonar.inclusions`
- 索引文件数：802
- Scanner：`EXECUTION SUCCESS`
- CE task：`0e8ef213-f7ed-4d05-8dbf-842da76d2dd2`
- CE：`SUCCESS`
- Analysis：`09735c32-b9e6-4fc8-9054-3891f4f44795`
- 日志：`.iteration/artifacts/sonar-full-wave.log`

随后用仓库相对 POSIX LCOV 重扫：scanner 仍 `EXECUTION SUCCESS`，但 CE task `f094467a-3bfb-444d-9623-9409f2f26d46` 为 `FAILED`，原因是 Sonar 已有 test component `packages/remote/src/shared/terminal/terminalFeedPolicy.test.ts` 记录 92 行，而当前源为 81 行。该失败 task 不覆盖上一个成功分析的指标；日志：`.iteration/artifacts/sonar-full-wave-normalized.log`。

复测波次 2 在同一完整配置下重新上传 802+ 文件：扫描器在生成报告后超过 604 秒工具上限，未形成可接受 scanner/CE 结果；本次一次性 token 已撤销，残留 scanner 子树已核验清理，Sonar 服务进程未动。日志：`.iteration/artifacts/sonar-full-wave-2.log`。该波次不改变下方已接受项目指标。

## 项目指标

- `coverage=40.3%`
- `line_coverage=41.2%`
- `branch_coverage=38.9%`
- `violations=841`
- `alert_status=ERROR`

Quality Gate 条件：`new_coverage=47.6` 未达 `80`；`new_violations=133` 未达 `0`。故本轮不能声称 Sonar 80% 或 Quality Gate 完成。

## 根因与下一步边界

- 前端 V8 报告已扩大到全 `src`、`packages/remote`、`scripts`，最新复测为 164 files、1649 passed、1 skipped；当前基线为 statements `43.64%`、branches `38.79%`、functions `44.53%`、lines `46.17%`；仍低于目标。日志：`.iteration/artifacts/vitest-coverage-wave-2.log`。
- Sonar 项目源还包含 Rust；当前仅接入 `sonar.javascript.lcov.reportPaths`，未形成 Rust LCOV，因此项目级指标不能由前端 V8 代替。
- 已尝试安装 `llvm-tools-preview` / `cargo-llvm-cov`，安装命令 10 分钟超时；确认无残留本次 `rustup`/`cargo`/`rustc` 进程后停止。未写入 token、Cookie 或密码。
- 需后续生成真实 Rust LCOV，接入 `sonar.rust.lcov.reportPaths`，并继续补齐未覆盖前端模块；仍须以 Sonar project API 的 coverage 与 Gate 结果验收。

## 复测补充

- `ridge-mcp` 在同一进程内的并发幂等竞态已修复并由 8 线程确定性测试覆盖；这只提升通信正确性，不改变 Sonar 覆盖率结论。
- pane-tree px-anchor 纯逻辑测试定向 75/75 通过；全量测试/覆盖复测未改变 Sonar 已接受的项目指标。
- Hub 新增 deadline 过期拒绝/回收与 `ridge_cancel_delivery` 取消穿透；SQLite 重开后 cancellation、receipt、inbox 状态一致。`ridge-mcp` 当前 `81/81` 通过；该 Rust 通信增量尚未生成新的 Sonar accepted analysis，故不改变项目级指标。
- 工作区 `cargo test --workspace --all-targets --quiet` 本轮在 Tauri 全量编译阶段达到 300 秒工具上限，未形成 test result；日志：`.iteration/artifacts/cargo-workspace-all-targets.log`。
- `TerminalManager` 新增 `4/4` public API 单测；全量 Vitest coverage 当前 `165 files / 1654 passed / 1 skipped`，statements `47.36% (8767/18509)`、branches `42.23% (4860/11508)`、functions `47.49% (1676/3529)`、lines `50.38% (7961/15800)`。该 V8 波次提升本地前端基线，但尚未形成新的 Sonar accepted analysis，不能替代项目级 80% 验收；日志：`.iteration/artifacts/vitest-coverage-wave-4-manager.log`。

## 覆盖波次 13 与 Sonar 认证结果

- 新增/扩充 `fsEvents`、`paneTree`、`fileWatcherSync`、`hosts` 真实行为测试；全量 Vitest 为 `168 files / 1655 passed / 1 skipped`。
- V8/LCOV 汇总：statements `52.42% (9703/18509)`、branches `47.03% (5413/11508)`、functions `54.34% (1918/3529)`、lines `55.59% (8784/15800)`；LCOV：`coverage/lcov.info`；日志：`.iteration/artifacts/vitest-coverage-wave-13-fs-pane-lcov.log`。
- 使用完整 `sonar-project.properties` 重新启动 scanner，未到分析阶段即因 Sonar `/api/v2/analysis/version` 返回 `401 Unauthorized` 退出码 1；环境变量 `SONAR_TOKEN` 缺失，未打印、生成或保存凭据。日志：`.iteration/artifacts/sonar-scan-wave-13.log`。
- 因本波没有 scanner/CE 成功结果，Sonar accepted 指标仍只引用首个完整成功扫描：coverage `40.3%`、line `41.2%`、branch `38.9%`、Quality Gate `ERROR`；不得用本地 V8 52.42% 替代项目级 80% 验收。

本轮未向 Codex 之外 CLI 派发消息，未提交、推送或发布。
