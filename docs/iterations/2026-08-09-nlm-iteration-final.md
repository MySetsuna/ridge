# 2026-08-09 NLM 迭代收敛记录

## 范围与来源

- 需求：`REQ-NLM-ITERATION-01`，已通过 requirements gate。
- NotebookLM：`Ridge 项目现状、愿景与规划基线（2026-07-21）`。
- 读取最近对话：10 turns；`sources_used: []`，故 NLM 结果仅作候选假设，未据此宣称已证实缺陷。
- 未向 NotebookLM 写回笔记、未发布、未 push、未创建 Release。

## 本轮落地

### Remote / workspace / pane

- `CloudHostTopologyLink` 保存并传递 `activeWorkspaceId`；`listWorkspacePanes`、`createPane`、切换/创建 workspace 均显式带 workspace identity。
- `cloudRemote` 在无 active workspace 时不再误刷新；创建 pane 使用指定 workspace 的 layout 与 split 参数。
- 已补 cloud topology、cloud remote 回归；Remote 相关焦点测试 `8 files / 139 passed`。

### ridge-term

- 共享 glyph atlas 的 `GlyphKey` 纳入量化 raster DPR，避免不同 pane / DPR 复用错误 glyph bitmap。
- WebGPU 各 glyph 路径统一使用实际 raster density。
- `cargo test -p ridge-term --lib`：`399 passed`；`cargo fmt --all -- --check`：通过。

### Explorer

- 文件树刷新路径统一 Windows 分隔符，覆盖跨 workspace、重复 cwd 与祖先路径刷新。
- 新增确定性测试；Explorer 目标测试 `2 files / 51 passed`。
- 真实跨卷权限失败尚未在物理环境复现，不能宣称已完成该现场问题。

### ptyBridge / Sonar 新问题

- 将嵌套条件表达式改为等价的显式分支，保留 `Uint8Array` 零拷贝与其他 payload 行为。
- `ptyBridge.test.ts`：`18 passed`；Sonar 重扫后 `new violations: 0`。

## 质量闸

- 全量 Vitest：`155 test files passed`，`1608 passed`，`1 skipped`。
- 覆盖率刷新：Statements `53.81%`，Branches `39.84%`，Functions `62.45%`，Lines `56.54%`。
- `svelte-check`：`0 errors, 0 warnings`。
- `cargo fmt --all -- --check`：通过。
- `git diff --check`：通过；已有工作区修改均保留，未清理用户内容。

## Sonar

- 本地 SonarQube `26.7.0.124771` 已启动于 `http://127.0.0.1:9000`，scanner `8.0.1.6346` 执行成功。
- 最新任务：`d7682a3c-42b9-4562-b93e-befe5a9dadb1`。
- 扫描任务 SUCCESS；Quality Gate ERROR 仅因 dirty baseline 下 new coverage `0.2%`（`7257` new lines to cover，`7249` uncovered），非新增静态违规。
- 最新扫描另有 `src/lib/stores/paneTree.ts:48` 非 UTF-8 警告；未擅改历史文件编码。
- 浏览器监控页未打开：当前 Codex browser connector 报 `No browser is available`，故只保留 API/CLI 证据。

## dev:cdp / E2E

- `tauri:dev:cdp` 已启动并到达真实 Ridge WebView2 CDP target：Edge `151.0.4129.72`，CDP `127.0.0.1:5878`。
- `cdp-smoke.mjs`：通过。
- `cdp-teammate-e2e.mjs`：`7/7` 通过。
- `cdp-dirchildren-probe.mjs C:/code/wind`：Host probe 通过；controller/transport 侧仍需完整 cloud/Postgres 环境。
- `cdp-pane-graph.mjs`、`cdp-pty-parsers.mjs` 均在 `set_remote_enabled` 处被 Windows Job Object 拒绝：
  `spawn detached process requires CREATE_BREAKAWAY_FROM_JOB for force-kill survival: 拒绝访问。 (os error 5)`。
  此为当前宿主 Job 限制，尚非 workspace handshake 的通过证据；未冒险改进程生命周期代码。
- CDP 进程树已按 PID 树清理。

## 未闭环项

以下仍需独立环境或用户现场证据，不能从本轮确定性测试推断完成：

1. 完整 cloud signaling/WebRTC/controller/host/Postgres 端到端接入。
2. 不受 Job Object 限制的 pane graph 与 PTY CDP E2E。
3. 不同 DPR 的物理截图/视觉验收。
4. Windows 跨卷、权限失败的真实文件操作验收。
5. 新 profile 下 mobile `runtime.lastError`、后台存活/重连现场复现。
6. Sonar new-coverage 基线治理与 `paneTree.ts` 编码警告处理。

