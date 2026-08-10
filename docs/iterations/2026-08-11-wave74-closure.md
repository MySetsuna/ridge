# Wave 74 闭环记录（2026-08-11）

## 结论

本轮已闭合的本地代码项：Agent Hub 的 PTY 五条件采样边界、generation/lease 围栏、PTY 释放清理、Remote/ridge-term 元数据链路、HMR stale-port 修复，以及对应 Rust/TypeScript 测试。五条件的真实桌面运行时生产采样器尚未接入；因此 PTY fallback 仍按设计 fail-closed，不宣称 NLM 需求全部完成。

## NLM 需求核验

NotebookLM 仅作候选来源。对基线笔记“Agent 通信架构重构”做了定向查询：`GoalStore`、`GraphState`、`Postgres checkpointer`、`goal recovery` 不在该来源的需求文本中；该来源实际要求是 Rust/SQLite Message Hub、typed envelope、Agent Runtime Matrix 与受控 PTY fallback。CodeGraph 同时确认本仓已有 Hub、SQLite、envelope、delivery policy、generation/lease 与跨端 adapter，因此未引入无来源的大型新架构。

## 本轮实现

- `McpHost::pty_runtime_snapshot` 成为唯一 host 运行时采样接入口；缺采样即保持 fail-closed。
- 桌面 Tauri 新增 `publish_pty_runtime_snapshot`：严格校验 workspace/pane/agent 映射、live PTY、generation/lease、五个安全条件、`stateRevision` 与 `inputEpoch`；支持 camelCase 与 snake_case。
- 采样写入与 Hub delivery registry 同源；PTY teardown/release 时删除旧证明，避免 pane 复用后残留安全状态。
- Hub 在选择 delivery adapter 前读取 host 采样并注册当前 generation/lease；旧快照不能跨代继承。

## 本地未闭项

- 当前仓库没有可同时提供 prompt/foreground/approval/user-input 五项事实及 generation/lease 的现成生产状态源；本轮只落地受校验发布边界，未伪造采样器。下一轮需由真实 Agent runtime/桌面输入状态源提供单次快照，再接入该命令并补现场证据。

## 可复核证据

- `cargo fmt --all -- --check`：通过。
- `cargo test -p ridge-mcp --lib --quiet`：90 passed。
- `cargo test --manifest-path src-tauri/Cargo.toml --lib teammate::mcp::tests --quiet`：5 passed。
- CodeGraph `sync` 后复查：`publish_pty_runtime_snapshot → register_pty_runtime_snapshot`，`McpHost::pty_runtime_snapshot → enqueue_hub_entry`；未发现绕过 Hub 的 PTY fallback 调用路径。因无真实生产采样器，仍维持 fail-closed。
- SonarQube `http://127.0.0.1:9000` API：server `UP`，project `MySetsuna_ridge` 最新分析 `9abdb231-3503-4f6e-b8ee-48d28f7086dc`；coverage `80.4%`、line coverage `86.7%`、branch coverage `71.5%`、Quality Gate `OK`、`new_violations=0`。临时 token 已撤销，未写入仓库。
- 本轮另一次隔离 scanner 运行在 Rust sensor 超过宿主 10 分钟后被精确清理；该失败尝试不作为 Sonar 成功证据，以上 API 最新分析才是权威结果。

## 仍需外部验收

- 实体手机上的可信 HTTPS/PWA、IME/键盘后台恢复与物理 DPR/像素矩阵。
- 第三方 CLI 的私有 Runtime/A2A 协议互操作；本仓只证明自有 endpoint/bridge 契约。
- Cloud/设备凭证驱动的现场链路；本地可复现 harness 与真实外部凭证边界分开记录。

发布、push、tag、Release 未执行；Sonar 管理账户密码仍由用户保管，文档不保存密码或 token。

## NLM 下一轮复核

最新查询仍将本轮划为“PTY 仅安全边界闭合、Remote 四路径需真实凭证/设备、ridge-term 需物理 DPR/像素矩阵、workspace/Remote 需双窗口与深根模式、移动连续性需真机”。仓库证据已补齐本地 LAN/移动 harness 与 DPR `1.5`，但不替代上述现场验收。NLM 对 Sonar 的旧记录认为质量门未运行；本轮以本地 Sonar API 最新分析为准，已取得 Gate `OK` 与 `new_violations=0`，不采信旧数值。

## CDP E2E 补充

- `node scripts/cdp-pty-parsers.mjs`：通过；冷启动 PTY 的 UTF-8、OSC title、OSC cwd 与 live binary frame 均有证据。
- `node scripts/cdp-lan-probe.mjs`：通过；hello、pane UUID、scrollback/live frame、唯一 echo 与 pong 均通过。
- `node scripts/cdp-dpr-e2e.mjs`：通过；`dpr=1.5`，canvas/backing canvas 均存在。
- `node scripts/cdp-remote-mobile-agents.mjs`：通过（114.7s）；真实 `/verify`、移动 SPA、数据面命令、Team/终端能力降级与 shell 入口均完成验证。
- 早先移动命令行超时源于调用方等待上限，不是产品断言失败；残留 Playwright Chrome 已按精确 PID 树清理。`tauri-dev-cdp` 已固定 `CARGO_TARGET_DIR`、显式 `--bin rdg`，并拒绝时间戳早于 LAN host 源码的 stale sidecar。

## Sonar 扫描环境诊断补充

- 权威项目 API 仍为 analysis `9abdb231-3503-4f6e-b8ee-48d28f7086dc`：Quality Gate `OK`、coverage `80.4%`、line `86.7%`、branch `71.5%`、`new_violations=0`、开放 issue `1`。唯一开放项为历史 `Web:PageWithoutTitleCheck`；当前 `src/remote/index.html` 已有 `<title>`，需后续成功上传一次分析刷新旧 issue。
- 为重扫清理出的环境首因：`scripts/perf-runs` 中 356MB 轨迹文件已纳入排除；Rust Clippy 开关实为 `sonar.rust.clippy.enabled`；JS/TS bridge 在 1.7GB heap 时明确 OOM，4GB 运行可完成 480 文件分析，但本机资源/计划任务回收使后续上传未稳定闭合。临时 token 均已撤销，未写入仓库。
- 该环境诊断不改变 Quality Gate、不扩大产品源排除；不得把未上传 scanner 日志作为新的 Sonar 成功证据。
