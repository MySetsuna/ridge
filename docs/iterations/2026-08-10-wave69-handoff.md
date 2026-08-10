# Wave69 现场验收与 Sonar 扫描交接

日期：2026-08-10

## Sonar 环境修复

- SonarQube `26.7.0.124771` 已恢复 `UP`；Elasticsearch cluster `green`。
- Sonar `data` 已复制并校验到 `D:\ridge-sonarqube-data-20260810`，共 `244` 文件、`535057673` bytes，源/目标一致；配置写入本机 `.tools/sonarqube-26.7.0.124771/conf/sonar.properties`，不属于仓库提交。
- 根因是 C: 剩余约 `6.6%` 触发 Elasticsearch flood-stage，`projectmeasures` 进入只读；迁移后新索引未再出现只读块。

## 覆盖率与扫描结果

- `pnpm test:coverage:sonar` exit `0`；本地 V8/LCOV：Statements `70.99%`，Branches `62.60%`，Functions `72.62%`，Lines `74.91%`。
- 完整主项目扫描曾因 Sonar JS 传感器在 Windows 首次分析停于 JS/TS bridge，已精确回收扫描进程并撤销临时 token；未将超时冒充成功。
- 配置修复：不再声明不存在的 `coverage/rust.lcov`，避免 Rust 传感器递归遍历失效 worktree junction；扫描范围收窄至 `src,packages,src-tauri/src`，排除 `scripts/` 与 `*.svelte` 的不稳定 Sonar JS 分析输入；本地覆盖率仍以完整 Vitest LCOV 为准。
- 独立 Rust 核心 scanner smoke：scanner exit `0`、CE `SUCCESS`、Quality Gate `OK`；项目 `MySetsuna_ridge-wave61-rust` 指标为 `0` bugs、`0` vulnerabilities、`44` code smells、重复率 `1.2%`。该项目不是全项目 80% 证明。
- 主项目 `MySetsuna_ridge` 的 `>=80%` coverage 仍未闭环；不以 Rust-only Gate 或本地 LCOV 冒充全项目 Sonar 达标。

## 仍开放的现场证据

- 新鲜 WebView2 `RIDGE_CDP_DEVICE_SCALE_FACTOR=1.5` 运行已通过：`dpr=1.5`、`canvasCount=3`、`backingCanvasCount=3`；截图产物为 `.iteration/artifacts/dpr/desktop-dpr15-shot.png`。首次冷启动探针曾在应用 ready 前超时，待应用实际挂载后重跑通过，记录为 harness 启动竞态，非业务断言失败。
- Kernel 深根现场新增结论：隔离 `kernel-host-smoke.ps1` 全部通过；真实 dev 外壳树被 `taskkill /T` 后，隔离 Kernel `PID 129944` 仍 `health=ok`。此前 harness 无条件注入 `RIDGE_TEST_ALLOW_NON_BREAKAWAY=1` 时复现“同 Job 连带杀 Kernel”，已修为显式 `RIDGE_CDP_ALLOW_NON_BREAKAWAY=1` 才允许 fallback，并补 `3/3` 单测；新 Tauri 外壳 attach 同一 PID 的完整重启仍待补证。
- Tauri 重启现场还暴露 Windows 文件锁边界：存活的 `rdg --ridge-kernel-host` 会锁 `target/debug/rdg.exe`，导致下一轮 dev harness 的 `cargo build -p ridge-cli` 收到 `os error 5`。现已让 `tauri-dev-cdp.mjs` 构建 `ridge` 后复制每轮独立 Kernel binary，并由 `RIDGE_KERNEL_HOST_BINARY` 注入桌面端；可解除旧 Kernel 对共享 desktop binary 的锁，但新窗口 attach 仍待现场补证。

- Tauri WebView2 外壳真实退出后，Kernel 同 PID 仍存活；当前已得真实 `taskkill /T` 现场证据，但新窗口 attach 同 PID 尚未完整通过，故不宣称产品生命周期闭环。
- 跨卷 `move_path` 在真实 copy/delete 窗口内注入 ACL；当前已证明 copy 成功后真实 `delete_path` 收到 Windows `os error 5` 且源/目标保留，未宣称未改生产 `move_path` 的中窗注入已通过。
- 真实物理 iOS/Android、可信 HTTPS PWA、后台恢复、IME/键盘锚点；外部 Chrome CDP shell/profile 流程已通过，不能替代真机。
- Sonar 全项目 coverage `>=80%`；当前本地基线距目标仍有差距，需求保持 `ACTIVE`。

## 凭据与接管

- Sonar 地址、账户与本地使用方式见 [SonarQube 交接文档](2026-08-09-sonarqube-handoff.md)。文档含本机默认账户提醒；生产/局域网开放前必须改密。
- 所有 scanner token 均临时生成、扫描后撤销；仓库未保存 token、cookie 或 NLM 内容。
- 未执行 push、tag、Release、Remote/Cloud 发布。

## NLM 下一批只读审计与本地裁决

- Notebook `66919cb9-1329-4ddf-955c-f426d15a9fe6`，sources `9516749e-c317-4f13-9cda-b64b00cec465`、`be660734-15ce-4e2e-8843-5430302c3a29`、`15441f90-cb8e-4cbe-b644-80ac68984653`；查询复用 conversation `a47d3199-c1f9-47f1-927c-ff2c4875b77d`，只读，无 source/note 改动。
- 候选为：Message Hub/PTY fallback、DPR 1.25/1.5 原生 PowerShell 对照、Codex 录制帧单调性、移动 PWA 后台/IME、跨卷部分失败、Sonar 全项目质量门、桌面外壳退出后的 Kernel 接管。
- CodeGraph/本地证据裁决：Message Hub、PTY safety、frameId 防旧帧、移动端复合身份/有界队列、Explorer DTO 与 Kernel lifecycle 已有代码和确定性测试；不重复造第二套实现。
- 本轮可继续落地者只剩外部验收：真实 Tauri WebView2 退出重接、真机 PWA/IME、原生 PowerShell 像素矩阵、跨卷中窗 ACL 注入、全项目 Sonar `>=80%`。这些不以模拟测试冒充闭环，登记为下一轮 user-track。
- 本次 NLM 只读复问仍返回同一活跃 `conversation_id=a47d3199-c1f9-47f1-927c-ff2c4875b77d`（当前 MCP 未暴露独立 `chat_start`）；其新增建议仍为 Message Hub、非整数 DPR、移动 PWA、跨卷 ACL、Sonar Gate 与 Kernel 深根，均已由本地 CodeGraph/测试裁决或登记为外部证据，不新增重复代码。

## Wave71 质量复验（2026-08-10）

- 标准 `pnpm test:coverage:sonar` 在并发覆盖率临时文件清理处失败：`ENOENT coverage/.tmp/coverage-196.json`；未将该半成品当作基线。
- 串行隔离覆盖率复验通过：`pnpm exec vitest run --coverage --coverage.processingConcurrency=1 --no-file-parallelism --coverage.reportsDirectory=.iteration/artifacts/coverage-wave71`，Statements `74.52%`、Branches `66.63%`、Functions `76.10%`、Lines `78.73%`；LCOV 经 `scripts/normalize-lcov.mjs` 规范化成功。
- 主项目 Sonar 扫描已真实上传并由 CE `SUCCESS` 处理：最新任务 `3c78dac8-13b1-41a8-a65e-cdce622ee4cd`。项目指标为 coverage `80.2%`、line `86.4%`、branch `71.6%`、violations `730`（bugs `17`、vulnerabilities `21`、code smells `692`）。Quality Gate 仍 `ERROR`，因 `new_violations=152` 未归零；不能宣称质量门通过。
- 本次扫描在隔离快照中仍报告缺失 `.svelte-kit/tsconfig.json` 的解析告警及无 SCM provider；扫描器/CE 终态成功，不影响上述指标，但真实 Tauri WebView2 重接、真机 PWA/IME、原生像素矩阵、跨卷中窗 ACL 注入、Sonar 全项目质量门仍属外部未闭环项。
- 根据 Sonar 低风险告警，故障注入 rig 已移除空方法：记录 pane 字节、生命周期状态与 ICE candidate，并新增确定性单测；`faultInjection.test.ts` 与质量 helper 定向合计 `33/33` 通过。远端 E2E harness 的高认知复杂度历史项未作 speculative 大重构。

## NLM Wave71 只读复问

- 仍复用同一 `conversation_id=a47d3199-c1f9-47f1-927c-ff2c4875b77d`，MCP 代理配置有效；未改 notebook、source 或 note。
- 新批次优先级：P0 Message Hub/Inbox/Delivery Engine 取代 PTY 字符注入为主协议；P0 ridge-term 非整数 DPR 物理对照与 Codex 录制逐帧单调性；P0 移动 PWA 后台/换网/IME/SW 真机闭环；P1 Workspace/Explorer 跨卷部分失败原子性；P1 Sonar 服务端 Gate 闭环。
- NLM 对 Sonar 覆盖率的旧引用仍为历史快照，已由本轮真实 CE `SUCCESS` 与项目 API 的 `80.2%` coverage / Gate `ERROR` 裁决；不重复实现 Message Hub 或虚构真机/物理证据。

## 新发现：Kernel PTY E2E 瞬态失败

- 一次并行 `cargo test -p ridge-cli --test kernel_lifecycle_e2e` 曾在 `kernel_pty_survives_client_detach_and_replays_after_cursor` 的 `poll_domain_pty_output` 处报 `malformed kernel response`；同一测试精确重跑、串行全量、随后并行全量均通过（分别 `1/1`、`3/3`、`3/3`）。
- 当前判定为未能复现的 Windows kernel HTTP/进程时序 flaky，已登记下一轮；不以重跑绿灯抹去首次失败，也不在无根因证据时改协议解析。

## Wave72：最终质量回归与 CodeGraph 复核（2026-08-10）

- 质量提交：`08d8c1c6`，覆盖 remote/cloud、pane/terminal、workspace 路径的 Sonar 机械清理、随机回退护栏、异步清理与对应断言；未提交 `.iteration/*`、`coverage/*` 或运行时凭据。
- 最终回归：`pnpm test` 为 `207` 个文件、`1943 passed / 1 skipped`；`pnpm check` 为 `0 errors / 0 warnings`；`pnpm build` exit `0`，Vite `4222 modules`，仅既有动态导入/包体警告。
- CodeGraph 复核确认 CloudHostPaneSource、ControllerCloudProvider、PaneRpcScheduler、TerminalManager geometry、workspaceScope 与构建门禁仍有调用链；`paneFeedScheduler.schedule` 仅能从公开 API 间接关联，已补“单帧单次调度”与“dispose 后回调失效”两条确定性测试。
- Sonar 当前项目指标仍为 coverage `80.2%`、line `86.4%`、branch `71.6%`、violations `730`，Quality Gate `ERROR`（`new_violations=152`）；不宣称质量门闭环。
- Sonar admin 密码已由用户轮换并经 `/api/authentication/validate` 验证；密码不写入仓库、日志或交接文档，仅保留“当前用户管理密码”用法说明。

## Wave72 CDP 复核：新增启动阻塞

- 重新启动 `pnpm tauri:dev:cdp` 时，`ridge-cli` 与 desktop Rust 构建均完成；WebView2 未启动，`beforeDevCommand` 因 `scripts/start-vite-dev.mjs:19` 访问 `target._events` 时 `target` 为 `undefined` 而以 exit `1` 结束（Node `v25.9.0`）。
- 已回收本次 CDP 进程树；未触碰已安装 Ridge。该问题登记为下一迭代 `BUG-CDP-START-VITE-TARGET-01`，暂不宣称本轮 CDP smoke 成功；上一轮历史 CDP 证据仍仅作历史证据。
